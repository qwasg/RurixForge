import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import UserMessageCard from '@/components/chat/UserMessageCard';
import AssistantMessage from '@/components/chat/AssistantMessage';
import ActivitySegment from '@/components/chat/ActivitySegment';
import SubagentRow from '@/components/chat/SubagentRow';
import SubagentOverlay from '@/components/chat/SubagentOverlay';
import MarkdownFlat from '@/components/chat/MarkdownFlat';
import MessageList from '@/components/chat/MessageList';
import CommandBlock from '@/components/chat/CommandBlock';
import FileChangeBlock from '@/components/chat/FileChangeBlock';
import type { ChatBlock } from '@/lib/timeline';
import { useUltraPlanStore, type UltraPlanState } from '@/lib/ultraPlanStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

/** F7 wave.4 消息组件渲染+交互测试(UserMessageCard/AssistantMessage/ActivitySegment 等)。 */

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialToasts = useToastStore.getState();

function userMsg(over: Partial<ChatMsg> = {}): ChatMsg {
  return {
    id: 'evt_u1',
    role: 'user',
    text: '给场景加碰撞体',
    blocks: [],
    status: 'completed',
    mode: 'build',
    time: '10:24',
    runId: 'run_1',
    ...over,
  };
}

function toolBlock(over: Partial<Extract<ChatBlock, { kind: 'tool' }>> = {}): ChatBlock {
  return {
    kind: 'tool',
    toolCallId: 'call_1',
    name: 'mcp__engine-scene__entity_create',
    args: JSON.stringify({ name: 'e1' }, null, 2),
    ok: true,
    durationMs: 12,
    mcp: ['engine-scene', 'entity_create'],
    ...over,
  };
}

beforeEach(() => {
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useToastStore.setState(initialToasts, true);
});

afterEach(() => {
  cleanup();
});

describe('Codex 原生图片预览', () => {
  it('图片独立于折叠过程显示，可放大和保存，不报空回复', () => {
    render(<AssistantMessage msg={{
      id: 'native-image', role: 'assistant', text: '', status: 'completed', time: '', engine: 'codex',
      blocks: [{ kind: 'image', toolCallId: 'image-call', url: '/api/forge/gen/image/file?fileRef=.forge/tmp/gen/image.png', imageFileRef: '.forge/tmp/gen/image.png' }],
    }} />);
    expect(screen.getByRole('img', { name: 'Codex 生成的图片' })).toHaveAttribute('src', '/api/forge/gen/image/file?fileRef=.forge/tmp/gen/image.png');
    expect(screen.getByRole('link', { name: '保存图片' })).toHaveAttribute('download', 'image.png');
    expect(screen.queryByTestId('assistant-empty')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '放大生成的图片' }));
    expect(screen.getByRole('dialog', { name: 'Codex 生成的图片' })).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('design-lightbox-close'));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
});

describe('<UserMessageCard />', () => {
  it('通栏卡:HH:MM 头 + 正文 + mode chip', () => {
    render(<UserMessageCard msg={userMsg()} />);
    expect(screen.getByText('10:24')).toBeInTheDocument();
    expect(screen.getByText('给场景加碰撞体')).toBeInTheDocument();
    expect(screen.getByText('Agent')).toBeInTheDocument(); // build → Agent
  });

  it('点击进编辑 → 改文本 → 重发 = editAndResend(id, newText, mode);取消回退', () => {
    const editAndResend = vi.fn();
    useChatStore.setState({ editAndResend });
    render(<UserMessageCard msg={userMsg()} />);
    fireEvent.click(screen.getByTestId('user-message-text'));
    const input = screen.getByTestId('user-edit-input');
    expect(input).toHaveValue('给场景加碰撞体');
    expect(screen.getByText('编辑后重新发送，将回退此后的对话')).toBeInTheDocument();
    fireEvent.change(input, { target: { value: '改成加两个' } });
    fireEvent.click(screen.getByTestId('user-edit-resend'));
    expect(editAndResend).toHaveBeenCalledWith('evt_u1', '改成加两个', 'build');
    // 再次进入后取消
    fireEvent.click(screen.getByTestId('user-message-text'));
    fireEvent.click(screen.getByTestId('user-edit-cancel'));
    expect(screen.queryByTestId('user-edit-input')).not.toBeInTheDocument();
  });

  it('编辑态模式 chip 可点开菜单切换,重发带新模式', () => {
    const editAndResend = vi.fn();
    useChatStore.setState({ editAndResend });
    render(<UserMessageCard msg={userMsg()} />);
    fireEvent.click(screen.getByTestId('user-message-text'));
    const modeBtn = screen.getByTestId('user-edit-mode');
    expect(modeBtn).toHaveTextContent('Agent');
    fireEvent.click(modeBtn);
    const menu = screen.getByTestId('user-edit-mode-menu');
    expect(menu).toBeInTheDocument();
    fireEvent.click(within(menu).getByTestId('user-edit-mode-ask'));
    expect(modeBtn).toHaveTextContent('Ask');
    fireEvent.click(screen.getByTestId('user-edit-resend'));
    expect(editAndResend).toHaveBeenCalledWith('evt_u1', '给场景加碰撞体', 'ask');
  });

  it('运行中禁止编辑(toast 提示)', () => {
    useChatStore.setState({ activeRunId: 'run_x' });
    render(<UserMessageCard msg={userMsg()} />);
    fireEvent.click(screen.getByTestId('user-message-text'));
    expect(screen.queryByTestId('user-edit-input')).not.toBeInTheDocument();
    expect(useToastStore.getState().items.some((t) => t.title.includes('运行中'))).toBe(true);
  });

  it('D-038 回执唤醒卡:source=receipt → 「回执唤醒」chip,不可编辑重发', () => {
    const editAndResend = vi.fn();
    useChatStore.setState({ editAndResend });
    render(
      <UserMessageCard
        msg={userMsg({ text: '【系统唤醒】后台子代理回执送达(2 条)…', mode: 'multitask', source: 'receipt' })}
      />,
    );
    const card = screen.getByTestId('user-message-card');
    expect(card).toHaveAttribute('data-source', 'receipt');
    expect(screen.getByTestId('user-wake-chip')).toHaveTextContent('回执唤醒');
    expect(screen.getByText('Multitask')).toBeInTheDocument();
    // 系统生成的话不是用户说的:没有编辑入口,点正文也不进编辑态。
    expect(screen.queryByTestId('user-edit-enter')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('user-message-text'));
    expect(screen.queryByTestId('user-edit-input')).not.toBeInTheDocument();
    expect(editAndResend).not.toHaveBeenCalled();
    // 普通用户卡仍标 user,且照旧有编辑入口。
    cleanup();
    render(<UserMessageCard msg={userMsg()} />);
    expect(screen.getByTestId('user-message-card')).toHaveAttribute('data-source', 'user');
    expect(screen.getByTestId('user-edit-enter')).toBeInTheDocument();
  });

  /** D-044:编辑重发 = 回退事件日志,而 UltraPlan 阶段机不随日志回退——流程内的消息一律不给编辑入口。 */
  const ULTRA_LOCK_TITLE = 'UltraPlan 流程内的消息不能回退,请用「重新开始」';
  const ultraFlow = (patch: Partial<UltraPlanState> = {}): UltraPlanState => ({
    id: 'up_a1',
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
  });

  it('D-044 卡片动作的回显(ultraAction):不可编辑重发,悬停说明改用「重新开始」;仍可复制', () => {
    const editAndResend = vi.fn();
    useChatStore.setState({ editAndResend });
    render(
      <UserMessageCard
        msg={userMsg({
          text: '已提交问卷(5 题)',
          mode: 'ultraplan',
          ultraAction: { id: 'up_a1', action: 'answer', rev: 1 },
        })}
      />,
    );
    const card = screen.getByTestId('user-message-card');
    expect(card).toHaveAttribute('data-locked', 'ultraplan');
    // 不是回执唤醒卡:来源仍是 user,没有「回执唤醒」chip,mode chip 照常
    expect(card).toHaveAttribute('data-source', 'user');
    expect(screen.queryByTestId('user-wake-chip')).not.toBeInTheDocument();
    expect(screen.getByText('UltraPlan')).toBeInTheDocument();
    const text = screen.getByTestId('user-message-text');
    expect(text).toHaveAttribute('title', ULTRA_LOCK_TITLE);
    expect(text.className).not.toContain('cursor-pointer');
    expect(screen.queryByTestId('user-edit-enter')).not.toBeInTheDocument();
    fireEvent.click(text);
    expect(screen.queryByTestId('user-edit-input')).not.toBeInTheDocument();
    expect(editAndResend).not.toHaveBeenCalled();
    // 被锁不是因为有任务在跑:不弹「运行中」提示
    expect(useToastStore.getState().items).toHaveLength(0);
    expect(screen.getByTestId('user-copy')).toBeInTheDocument();
  });

  it('D-044 制作动作的回显走 team 模式:同样锁住(看的是 ultraAction,不是 mode)', () => {
    render(
      <UserMessageCard
        msg={userMsg({
          text: '确认计划,开始制作',
          mode: 'team',
          ultraAction: { id: 'up_a1', action: 'start_production', rev: 1 },
        })}
      />,
    );
    expect(screen.getByTestId('user-message-card')).toHaveAttribute('data-locked', 'ultraplan');
    expect(screen.queryByTestId('user-edit-enter')).not.toBeInTheDocument();
  });

  it('D-044 ultraplan 模式发出的消息:流程进行中锁住;没有流程 / 流程已完成 / 流程属于别的会话时照常可编辑', () => {
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    const brief = userMsg({ text: '做一个塔防小游戏', mode: 'ultraplan' });

    // 没有流程(流程还没建起来,或已被重新开始)
    const { unmount } = render(<UserMessageCard msg={brief} />);
    expect(screen.getByTestId('user-message-card')).not.toHaveAttribute('data-locked');
    expect(screen.getByTestId('user-edit-enter')).toBeInTheDocument();
    expect(screen.getByTestId('user-message-text')).toHaveAttribute('title', '点击编辑并重发');

    // 流程进行中
    act(() => useUltraPlanStore.getState().hydrate(ultraFlow(), 'sess_1'));
    expect(screen.getByTestId('user-message-card')).toHaveAttribute('data-locked', 'ultraplan');
    expect(screen.queryByTestId('user-edit-enter')).not.toBeInTheDocument();
    expect(screen.getByTestId('user-message-text')).toHaveAttribute('title', ULTRA_LOCK_TITLE);
    fireEvent.click(screen.getByTestId('user-message-text'));
    expect(screen.queryByTestId('user-edit-input')).not.toBeInTheDocument();

    // 流程已完成
    act(() =>
      useUltraPlanStore
        .getState()
        .hydrate(ultraFlow({ stage: 'done', demoIteration: 1, planRev: 1, acceptanceRound: 1 }), 'sess_1'),
    );
    expect(screen.getByTestId('user-message-card')).not.toHaveAttribute('data-locked');
    expect(screen.getByTestId('user-edit-enter')).toBeInTheDocument();

    // store 里挂的是别的会话的流程
    act(() => useUltraPlanStore.getState().hydrate(ultraFlow(), 'sess_other'));
    expect(screen.getByTestId('user-message-card')).not.toHaveAttribute('data-locked');
    unmount();

    // 流程进行中,但这条是别的模式发的普通消息:不受影响
    act(() => useUltraPlanStore.getState().hydrate(ultraFlow(), 'sess_1'));
    render(<UserMessageCard msg={userMsg()} />);
    expect(screen.getByTestId('user-message-card')).not.toHaveAttribute('data-locked');
    expect(screen.getByTestId('user-edit-enter')).toBeInTheDocument();
  });

  it('D-044 编辑到一半流程开始了:编辑态随之收起,不留没有出口的输入框', () => {
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    const editAndResend = vi.fn();
    useChatStore.setState({ editAndResend });
    render(<UserMessageCard msg={userMsg({ text: '做一个塔防小游戏', mode: 'ultraplan' })} />);
    fireEvent.click(screen.getByTestId('user-message-text'));
    expect(screen.getByTestId('user-edit-input')).toBeInTheDocument();
    act(() => useUltraPlanStore.getState().hydrate(ultraFlow({ stage: 'discovery', questionnaireRev: 0 }), 'sess_1'));
    expect(screen.queryByTestId('user-edit-input')).not.toBeInTheDocument();
    expect(screen.queryByTestId('user-edit-resend')).not.toBeInTheDocument();
    expect(screen.getByTestId('user-message-text')).toHaveTextContent('做一个塔防小游戏');
    expect(editAndResend).not.toHaveBeenCalled();
    // 收起即取消:流程结束、卡片解锁后,编辑框不会自己再弹出来
    act(() =>
      useUltraPlanStore
        .getState()
        .hydrate(ultraFlow({ stage: 'done', demoIteration: 1, planRev: 1, acceptanceRound: 1 }), 'sess_1'),
    );
    expect(screen.queryByTestId('user-edit-input')).not.toBeInTheDocument();
    expect(screen.getByTestId('user-edit-enter')).toBeInTheDocument();
  });
});

describe('<AssistantMessage />', () => {
  it('Codex 徽标 + plan/approval 里程碑，审批决定带 id 与 decision', async () => {
    const resolvePermission = vi.fn(async () => undefined);
    const openPlan = vi.fn();
    useChatStore.setState({ resolvePermission });
    useWorkbenchStore.setState({ openPlan });
    const msg: ChatMsg = {
      id: 'codex-1', role: 'assistant', text: '', status: 'streaming', time: '',
      engine: 'codex', model: 'gpt-5',
      blocks: [
        { kind: 'plan', text: '## 实施计划\n先取证', final: true, planPath: '.forge/plans/demo.plan.md' },
        {
          kind: 'approval', id: 'perm-1', approvalKind: 'command', command: 'cargo test',
          reason: '需要运行测试', availableDecisions: ['accept', 'acceptForSession', 'decline'],
        },
      ],
    };
    render(<AssistantMessage msg={msg} />);
    expect(screen.getByTestId('assistant-avatar')).toHaveTextContent('Codex');
    fireEvent.click(screen.getByTestId('plan-block-open'));
    expect(openPlan).toHaveBeenCalledWith('.forge/plans/demo.plan.md');
    fireEvent.click(screen.getByTestId('approval-session'));
    await act(async () => Promise.resolve());
    expect(resolvePermission).toHaveBeenCalledWith('perm-1', true, {
      decision: 'acceptForSession', answers: {},
    });
  });

  it('崩溃恢复后的历史审批显示已失效且不再可操作', () => {
    render(<AssistantMessage msg={{
      id: 'codex-expired', role: 'assistant', text: '', status: 'failed', time: '',
      engine: 'codex',
      blocks: [{
        kind: 'approval', id: 'perm-expired', approvalKind: 'command', tool: 'shell',
        decision: 'expired',
      }],
    }} />);
    expect(screen.getByTestId('approval-decision')).toHaveTextContent('已失效');
    expect(screen.queryByTestId('approval-accept')).not.toBeInTheDocument();
    expect(screen.queryByTestId('approval-decline')).not.toBeInTheDocument();
  });

  it('requestUserInput 必填秘密回答完成前不可批准，提交时保留答案', async () => {
    const resolvePermission = vi.fn(async () => undefined);
    useChatStore.setState({ resolvePermission });
    render(<AssistantMessage msg={{
      id: 'codex-input', role: 'assistant', text: '', status: 'streaming', time: '',
      engine: 'codex',
      blocks: [{
        kind: 'approval', id: 'perm-input', approvalKind: 'userInput', tool: 'request_user_input',
        questions: [{ id: 'token', header: '凭据', question: '请输入临时令牌', isSecret: true }],
        availableDecisions: ['accept', 'decline'],
      }],
    }} />);

    const accept = screen.getByTestId('approval-accept');
    const input = screen.getByTestId('approval-question-token');
    expect(input).toHaveAttribute('type', 'password');
    expect(accept).toBeDisabled();
    fireEvent.change(input, { target: { value: 'one-time-secret' } });
    expect(accept).not.toBeDisabled();
    fireEvent.click(accept);
    await act(async () => Promise.resolve());
    expect(resolvePermission).toHaveBeenCalledWith('perm-input', true, {
      decision: 'accept', answers: { token: 'one-time-secret' },
    });
  });

  it('MCP elicitation schema 渲染类型化表单；权限请求原文可审阅', async () => {
    const resolvePermission = vi.fn(async () => undefined);
    useChatStore.setState({ resolvePermission });
    const { rerender } = render(<AssistantMessage msg={{
      id: 'codex-form', role: 'assistant', text: '', status: 'streaming', time: '',
      engine: 'codex',
      blocks: [{
        kind: 'approval', id: 'perm-form', approvalKind: 'elicitation', message: '设置发布参数',
        schema: {
          type: 'object',
          properties: {
            enabled: { type: 'boolean', title: '启用发布' },
            retries: { type: 'integer', title: '重试次数', minimum: 0 },
          },
          required: ['enabled'],
        },
        availableDecisions: ['accept', 'decline'],
      }],
    }} />);
    const accept = screen.getByTestId('approval-accept');
    expect(screen.getByTestId('approval-schema-form')).toBeInTheDocument();
    expect(accept).toBeDisabled();
    fireEvent.change(screen.getByTestId('approval-schema-enabled'), { target: { value: 'false' } });
    fireEvent.change(screen.getByTestId('approval-schema-retries'), { target: { value: '2' } });
    fireEvent.click(accept);
    await act(async () => Promise.resolve());
    expect(resolvePermission).toHaveBeenCalledWith('perm-form', true, {
      decision: 'accept', answers: { enabled: false, retries: 2 },
    });

    rerender(<AssistantMessage msg={{
      id: 'codex-permissions', role: 'assistant', text: '', status: 'streaming', time: '',
      engine: 'codex',
      blocks: [{
        kind: 'approval', id: 'perm-scope', approvalKind: 'permissions', tool: 'permissions',
        permissions: { fileSystem: { read: ['D:/RurixForge'], write: [] }, network: null },
        grantRoot: 'D:/RurixForge/generated',
        networkApprovalContext: { host: 'api.example.com' },
        proposedExecpolicyAmendment: ['cargo', 'test'],
        proposedNetworkPolicyAmendments: [{ host: 'api.example.com', action: 'allow' }],
      }],
    }} />);
    expect(screen.getByTestId('approval-permissions')).toHaveTextContent('D:/RurixForge');
    expect(screen.getByTestId('approval-grant-root')).toHaveTextContent('D:/RurixForge/generated');
    expect(screen.getByTestId('approval-policy-amendments')).toHaveTextContent('api.example.com');
  });

  it('URL elicitation 仅开放 HTTP(S)，且完成外部流程前不可接受', () => {
    const msg = (url: string): ChatMsg => ({
      id: 'codex-url', role: 'assistant', text: '', status: 'streaming', time: '', engine: 'codex',
      blocks: [{
        kind: 'approval', id: 'perm-url', approvalKind: 'elicitation', mode: 'url',
        serverName: 'oauth-mcp', elicitationId: 'elicit-url', message: '完成外部授权', url,
        availableDecisions: ['accept', 'decline'],
      }],
    });
    const { rerender } = render(<AssistantMessage msg={msg('https://auth.example.com/start')} />);
    expect(screen.getByTestId('approval-external-link')).toHaveAttribute('href', 'https://auth.example.com/start');
    expect(screen.getByTestId('approval-accept')).toBeDisabled();
    fireEvent.click(screen.getByTestId('approval-external-confirm'));
    expect(screen.getByTestId('approval-accept')).not.toBeDisabled();

    rerender(<AssistantMessage msg={msg('javascript:alert(1)')} />);
    expect(screen.queryByTestId('approval-external-link')).not.toBeInTheDocument();
    expect(screen.getByTestId('approval-external-flow')).toHaveTextContent('已阻止打开');
    expect(screen.getByTestId('approval-accept')).toBeDisabled();
  });

  it('CommandBlock 显示实时输出/退出码；FileChangeBlock 按文件渲染 diff', () => {
    const { rerender } = render(
      <CommandBlock
        block={{
          kind: 'tool', toolCallId: 'cmd-1', name: 'shell', toolKind: 'command', mcp: null,
          args: JSON.stringify({ command: 'cargo test', cwd: 'D:/RurixForge' }),
          output: 'test result: ok', exitCode: 0, ok: true,
        }}
      />,
    );
    fireEvent.click(screen.getByTestId('command-block-cmd-1').querySelector('button')!);
    expect(screen.getByTestId('command-output')).toHaveTextContent('test result: ok');
    expect(screen.getByTestId('command-exit-code')).toHaveTextContent('exit 0');

    rerender(
      <FileChangeBlock
        block={{
          kind: 'tool', toolCallId: 'edit-1', name: 'apply_patch', toolKind: 'fileChange', mcp: null,
          args: '', ok: true,
          changes: [{ path: 'src/main.ts', kind: 'update', diff: '@@\n-old\n+new' }],
        }}
      />,
    );
    fireEvent.click(screen.getByTestId('file-change-block-edit-1').querySelector('button')!);
    expect(screen.getByText('src/main.ts')).toBeInTheDocument();
    expect(screen.getByText('+new')).toBeInTheDocument();
    expect(screen.getByText('-old')).toBeInTheDocument();
  });

  it('头:项目 logo + Agent + 模型 label + 时间;最终 text 走 markdown 不折叠', () => {
    const msg: ChatMsg = {
      id: 'a1',
      role: 'assistant',
      text: '',
      status: 'completed',
      model: 'mock',
      time: '10:25',
      runId: 'run_1',
      blocks: [
        toolBlock(),
        { kind: 'text', text: '# 完成\n已创建', final: true },
      ],
    };
    render(<AssistantMessage msg={msg} />);
    expect(screen.getByTestId('assistant-avatar')).toHaveAttribute('src', '/logo-transparent.png');
    expect(screen.getByTestId('assistant-avatar')).toHaveAttribute('alt', 'RurixForge');
    expect(screen.getByText('Agent')).toBeInTheDocument();
    expect(screen.getByTestId('assistant-model')).toHaveTextContent('mock');
    expect(screen.getByText('10:25')).toBeInTheDocument();
    // 最终回答完整 markdown:标题标记在解析阶段剥掉,不再以 `# ` 原样上屏
    expect(screen.getByTestId('markdown-flat').textContent).toContain('完成');
    expect(screen.getByTestId('markdown-flat').textContent).not.toContain('#');
    expect(screen.getByTestId('markdown-flat').className).toContain('font-bold');
    // 工具段:单行英文汇总不展开
    expect(screen.getByTestId('activity-segment').textContent).toContain('Edited 1 file');
  });

  it('完成但无正文:如实提示,不装成还在等', () => {
    const msg: ChatMsg = {
      id: 'a-empty',
      role: 'assistant',
      text: '',
      status: 'completed',
      time: '10:26',
      runId: 'run_empty',
      blocks: [{ kind: 'text', text: ' ', final: true }],
    };
    render(<AssistantMessage msg={msg} />);
    expect(screen.getByTestId('assistant-empty')).toHaveTextContent('模型没有返回正文');
  });

  it('D-044 以 UltraPlan 关口卡收尾的轮:卡片就是可见产出,不提示「模型没有返回正文」', () => {
    const ultra = (step: 'questionnaire' | 'demo' | 'plan' | 'acceptance' | 'done', rev: number): ChatBlock => ({
      kind: 'ultraplan',
      step,
      upId: 'up_a1',
      rev,
      payload: {},
    });
    const msg = (blocks: ChatBlock[]): ChatMsg => ({
      id: 'a-ultra',
      role: 'assistant',
      text: '',
      status: 'completed',
      time: '10:27',
      runId: 'run_ultra',
      blocks,
    });
    // 整轮只有过程链 + 一张问卷卡,没有任何正文
    const { unmount } = render(
      <AssistantMessage
        msg={msg([
          toolBlock({ name: 'ultraplan_questionnaire', args: JSON.stringify({ title: '塔防问卷' }), mcp: null }),
          ultra('questionnaire', 1),
        ])}
      />,
    );
    expect(screen.getByTestId('ultraplan-block')).toHaveAttribute('data-step', 'questionnaire');
    expect(screen.getByTestId('questionnaire-card')).toBeInTheDocument();
    expect(screen.queryByTestId('assistant-empty')).not.toBeInTheDocument();
    unmount();

    // 其余四种关口卡同理(含空白正文块)
    for (const block of [ultra('demo', 1), ultra('plan', 1), ultra('acceptance', 1), ultra('done', 0)]) {
      const view = render(<AssistantMessage msg={msg([{ kind: 'text', text: ' ', final: true }, block])} />);
      expect(screen.getByTestId('ultraplan-block')).toBeInTheDocument();
      expect(screen.queryByTestId('assistant-empty')).not.toBeInTheDocument();
      view.unmount();
    }

    // 对照:同样的轮去掉卡片 → 如实提示
    render(<AssistantMessage msg={msg([{ kind: 'text', text: ' ', final: true }])} />);
    expect(screen.getByTestId('assistant-empty')).toBeInTheDocument();
  });

  /// 2026-09-03 用户指令:正文强化 —— 中间叙述与最终回答同款(全黑加粗),
  /// 不再把中间叙述压成 text_2 弱文。
  it('中间叙述与最终回答同为加粗黑正文', () => {
    const msg: ChatMsg = {
      id: 'a2',
      role: 'assistant',
      text: '',
      status: 'completed',
      time: '',
      blocks: [
        { kind: 'text', text: '先看一下', final: false },
        toolBlock(),
        { kind: 'text', text: '最终', final: true },
      ],
    };
    const { container } = render(<AssistantMessage msg={msg} />);
    const flats = container.querySelectorAll('[data-testid="markdown-flat"]');
    expect(flats).toHaveLength(2);
    expect(flats[0].textContent).toBe('先看一下');
    expect(flats[1].textContent).toBe('最终');
    for (const flat of flats) {
      expect(flat.className).toContain('font-bold');
      expect(flat.className).toContain('text-fg');
      expect(flat.className).not.toContain('text-fg-2');
    }
  });

  /// 2026-09-03 用户指令「报错不需特别标明」:失败态不再红显(状态点退 dot-idle、
  /// 错误行退 text_3 灰),错误原文照旧如实上屏。
  /// D-047:运行中那一行自己扫光(取代原消息末 accent caret),此时不另挂轮次状态行。
  it('streaming:运行中的汇总行扫光、不叠状态行;failed 后停扫,错误行如实但不红显', () => {
    const streaming: ChatMsg = {
      id: 'a3', role: 'assistant', text: '', status: 'streaming', time: '',
      blocks: [toolBlock({ ok: undefined, durationMs: undefined })],
    };
    const { rerender } = render(<AssistantMessage msg={streaming} />);
    const seg = screen.getByTestId('activity-segment');
    expect(seg.textContent).toContain('Editing 1 file');
    expect(seg.querySelector('.forge-shimmer')).not.toBeNull();
    expect(screen.queryByTestId('turn-status')).not.toBeInTheDocument();
    rerender(
      <AssistantMessage
        msg={{ ...streaming, status: 'failed', error: '模板未命中', blocks: streaming.blocks }}
      />,
    );
    expect(screen.getByTestId('activity-segment').querySelector('.forge-shimmer')).toBeNull();
    expect(screen.queryByTestId('turn-status')).not.toBeInTheDocument();
    const err = screen.getByTestId('assistant-error');
    expect(err).toHaveTextContent('模板未命中');
    expect(err.className).not.toContain('danger');
  });

  /// D-047 Cursor 式等待显示:眼前没有正在执行的行时,消息末挂轮次状态行。
  it('轮次状态行:开轮即 Planning next moves;工具收尾去抖后出现,空档过久升级 Taking longer', () => {
    vi.useFakeTimers();
    try {
      const msg: ChatMsg = { id: 'a-st', role: 'assistant', text: '', status: 'streaming', time: '', blocks: [] };
      const { rerender } = render(<AssistantMessage msg={msg} />);
      const line = screen.getByTestId('turn-status');
      expect(line).toHaveAttribute('data-state', 'planning');
      expect(line.textContent).toBe('Planning next moves');
      expect(line.querySelector('.forge-shimmer')).not.toBeNull();

      // 工具在跑:让位给扫光的工具行
      rerender(<AssistantMessage msg={{ ...msg, blocks: [toolBlock({ ok: undefined })] }} />);
      expect(screen.queryByTestId('turn-status')).not.toBeInTheDocument();
      // 工具收尾:去抖窗口内不闪,过窗口出 Planning
      rerender(<AssistantMessage msg={{ ...msg, blocks: [toolBlock()] }} />);
      expect(screen.queryByTestId('turn-status')).not.toBeInTheDocument();
      act(() => {
        vi.advanceTimersByTime(500);
      });
      expect(screen.getByTestId('turn-status')).toHaveAttribute('data-state', 'planning');
      act(() => {
        vi.advanceTimersByTime(15_000);
      });
      const slow = screen.getByTestId('turn-status');
      expect(slow).toHaveAttribute('data-state', 'slow');
      expect(slow.textContent).toBe('Taking longer than expected · 15s');
      // 收束即撤
      rerender(<AssistantMessage msg={{ ...msg, status: 'completed', blocks: [toolBlock()] }} />);
      expect(screen.queryByTestId('turn-status')).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it('轮次状态行:未决审批 → Waiting for approval(静止不扫);事件流掉线超宽限 → Reconnecting', () => {
    const msg: ChatMsg = {
      id: 'a-ap', role: 'assistant', text: '', status: 'streaming', time: '',
      blocks: [{ kind: 'approval', id: 'perm-1', approvalKind: 'command', command: 'rm -rf build' }],
    };
    render(<AssistantMessage msg={msg} />);
    const line = screen.getByTestId('turn-status');
    expect(line).toHaveAttribute('data-state', 'approval');
    expect(line.textContent).toBe('Waiting for approval');
    expect(line.querySelector('.forge-shimmer')).toBeNull();
    act(() => {
      useChatStore.setState({ streamLink: 'down', streamDownSince: Date.now() - 2_000 });
    });
    expect(screen.getByTestId('turn-status')).toHaveAttribute('data-state', 'reconnecting');
    expect(screen.getByTestId('turn-status').textContent).toBe('Reconnecting');
    act(() => {
      useChatStore.setState({ streamLink: 'live', streamDownSince: null });
    });
    expect(screen.getByTestId('turn-status')).toHaveAttribute('data-state', 'approval');
  });

  it('流式中新块自上而下入场;历史(已收束)消息不播入场', () => {
    const base: ChatMsg = {
      id: 'a-in', role: 'assistant', text: '', status: 'streaming', time: '',
      blocks: [toolBlock(), { kind: 'text', text: '第一段', final: false }],
    };
    const { rerender, unmount } = render(<AssistantMessage msg={base} />);
    // 过程段整段入场;正文按 markdown 块入场(正文外层不叠一层)
    expect(screen.getByTestId('activity-segment').closest('.forge-stream-in')).not.toBeNull();
    const flat = screen.getByTestId('markdown-flat');
    expect(flat.parentElement?.classList.contains('forge-stream-in')).toBe(false);
    expect(flat.querySelectorAll('.forge-stream-in')).toHaveLength(1);
    rerender(<AssistantMessage msg={{ ...base, blocks: [toolBlock(), { kind: 'text', text: '第一段\n\n第二段', final: false }] }} />);
    expect(screen.getByTestId('markdown-flat').querySelectorAll('.forge-stream-in')).toHaveLength(2);
    unmount();

    render(<AssistantMessage msg={{ ...base, status: 'completed' }} />);
    expect(document.querySelector('.forge-stream-in')).toBeNull();
  });

  it('思考结束行「Thought {时长}」展开全文;无计时 Thought briefly', () => {
    const msg: ChatMsg = {
      id: 'a4', role: 'assistant', text: '', status: 'completed', time: '',
      blocks: [{
        kind: 'reasoning',
        text: '先分析问题\n再动手',
        startedTs: '2026-09-03T10:00:00.000Z',
        endedTs: '2026-09-03T10:00:07.000Z',
      }],
    };
    const { rerender } = render(<AssistantMessage msg={msg} />);
    const line = screen.getByTestId('reasoning-line');
    expect(line.textContent).toContain('Thought 7s');
    // 折叠时不剧透思考正文
    expect(line.textContent).not.toContain('先分析问题');
    fireEvent.click(line);
    // 展开后细节区出现全文(外层包装 div 与内层同 textContent,取 all 断言非空)
    expect(
      screen.getAllByText((_, el) => el?.textContent === '先分析问题\n再动手').length,
    ).toBeGreaterThanOrEqual(1);
    rerender(
      <AssistantMessage msg={{ ...msg, blocks: [{ kind: 'reasoning', text: '无计时' }] }} />,
    );
    expect(screen.getByTestId('reasoning-line').textContent).toBe('Thought briefly');
  });

  /// 进行中仍只显扫光「Thinking」不剧透摘录(2026-09-03 早前指令),
  /// 但本波按目标截图补上可展开 —— 想看实时思考流点一下即可。
  it('思考中显扫光 Thinking,默认不剧透、点开可看实时思考流', () => {
    const msg: ChatMsg = {
      id: 'a5', role: 'assistant', text: '', status: 'streaming', time: '',
      blocks: [{ kind: 'reasoning', text: '先分析问题', startedTs: '2026-09-03T10:00:00.000Z' }],
    };
    render(<AssistantMessage msg={msg} />);
    const line = screen.getByTestId('reasoning-line');
    expect(line.textContent).toBe('Thinking ');
    expect(line.querySelector('.forge-shimmer')).not.toBeNull();
    // 思考行自己在扫,不另挂轮次状态行
    expect(screen.queryByTestId('turn-status')).not.toBeInTheDocument();
    fireEvent.click(line);
    expect(screen.getByText('先分析问题')).toBeInTheDocument();
  });

  /// 目标截图:纯思考段不套「Working」汇总壳,裸行直出(正文上方那条 Thought briefly)。
  it('思考块并进活动段:与工具行同列;纯思考段不套汇总壳', () => {
    const mixed: ChatMsg = {
      id: 'a6', role: 'assistant', text: '', status: 'completed', time: '',
      blocks: [
        toolBlock({ toolCallId: 'r1', name: 'read_file', args: JSON.stringify({ path: 'src/lib/timeline.ts' }), mcp: null }),
        { kind: 'reasoning', text: '想一下' },
        { kind: 'text', text: '结论', final: true },
      ],
    };
    const { rerender } = render(<AssistantMessage msg={mixed} />);
    const seg = screen.getByTestId('activity-segment');
    expect(seg.textContent).toContain('Explored 1 file');
    fireEvent.click(seg);
    expect(screen.getByTestId('reasoning-line').textContent).toBe('Thought briefly');
    expect(screen.getByTestId('tool-line-r1').textContent).toBe('Read timeline.ts');
    // 纯思考段:无汇总壳,思考行直接可见
    rerender(
      <AssistantMessage
        msg={{ ...mixed, blocks: [{ kind: 'reasoning', text: '想一下' }, { kind: 'text', text: '结论', final: true }] }}
      />,
    );
    expect(screen.queryByTestId('activity-segment')).not.toBeInTheDocument();
    expect(screen.getByTestId('reasoning-line').textContent).toBe('Thought briefly');
  });
});

describe('<ActivitySegment />', () => {
  it('汇总行 +N/-N;展开见工具行;运行中首动词换现在分词', () => {
    const blocks: ChatBlock[] = [
      toolBlock({ toolCallId: 'c1', ok: true }),
      toolBlock({
        toolCallId: 'c2',
        name: 'mcp__code-forge__code_structured_edit',
        args: JSON.stringify({ path: 'a.rx', old_str: 'x', new_str: 'x\ny' }),
        mcp: ['code-forge', 'code_structured_edit'],
      }),
    ];
    render(<ActivitySegment blocks={blocks} indices={[0, 1]} />);
    const seg = screen.getByTestId('activity-segment');
    expect(seg.textContent).toContain('Edited 2 files');
    expect(seg.textContent).toContain('+1');
    fireEvent.click(seg);
    expect(screen.getByTestId('tool-line-c1')).toBeInTheDocument();
    expect(screen.getByTestId('tool-line-c2').textContent).toBe('Edited a.rx');
    // 失败 + 运行中:失败不再标红也不缀「n 失败」(2026-09-03 用户指令)
    const blocks2: ChatBlock[] = [
      toolBlock({ toolCallId: 'c3', ok: false, error: '炸了' }),
      toolBlock({ toolCallId: 'c4', ok: undefined, durationMs: undefined }),
    ];
    render(<ActivitySegment blocks={blocks2} indices={[0, 1]} />);
    const segs = screen.getAllByTestId('activity-segment');
    const seg2 = segs[segs.length - 1];
    expect(seg2.textContent).toContain('Editing 2 files');
    expect(seg2.textContent).not.toContain('失败');
    expect(seg2.className).not.toContain('danger');
    expect(seg2.querySelector('.text-danger')).toBeNull();
  });

  it('工具行展开细节:args mono + 完成 durationMs', () => {
    const blocks: ChatBlock[] = [toolBlock({ toolCallId: 'c9', ok: true, durationMs: 34 })];
    render(<ActivitySegment blocks={blocks} indices={[0]} />);
    fireEvent.click(screen.getByTestId('activity-segment'));
    fireEvent.click(screen.getByTestId('tool-line-c9'));
    expect(screen.getByTestId('tool-detail-args').textContent).toContain('"name": "e1"');
    expect(screen.getByText('Done · 34ms')).toBeInTheDocument();
  });

  /// 失败详情仍如实可见(展开即见错误原文),只是不再靠颜色喊话。
  it('失败工具行:行内不标记,展开后错误原文如实', () => {
    const blocks: ChatBlock[] = [toolBlock({ toolCallId: 'c7', ok: false, error: 'ENTITY_NOT_FOUND: e9' })];
    render(<ActivitySegment blocks={blocks} indices={[0]} />);
    fireEvent.click(screen.getByTestId('activity-segment'));
    const line = screen.getByTestId('tool-line-c7');
    expect(line.textContent).toBe('Created entity e1');
    fireEvent.click(line);
    expect(screen.getByTestId('tool-detail-error').textContent).toContain('ENTITY_NOT_FOUND: e9');
  });

  it('工具行展开显示 result', () => {
    const blocks: ChatBlock[] = [toolBlock({ toolCallId: 'c8', ok: true, durationMs: 9, result: 'wrote a.txt' })];
    render(<ActivitySegment blocks={blocks} indices={[0]} />);
    fireEvent.click(screen.getByTestId('activity-segment'));
    fireEvent.click(screen.getByTestId('tool-line-c8'));
    expect(screen.getByTestId('tool-detail-result').textContent).toContain('wrote a.txt');
  });

  /// D-047:「正在执行」的行整行扫光,且以消息仍在流式为前提(收束后残留 running 的块不扫)。
  it('运行态扫光:流式中运行的汇总行 / 工具行扫光,已完成的行与非流式不扫;展开后新到的行入场', () => {
    const running = toolBlock({ toolCallId: 'r1', name: 'read_file', args: JSON.stringify({ path: 'a.ts' }), mcp: null, ok: undefined });
    const done = toolBlock({ toolCallId: 'd1', name: 'grep', args: JSON.stringify({ query: 'x' }), mcp: null });
    const { rerender } = render(<ActivitySegment blocks={[done]} indices={[0]} streaming />);
    fireEvent.click(screen.getByTestId('activity-segment'));
    expect(screen.getByTestId('activity-segment').querySelector('.forge-shimmer')).toBeNull();
    expect(screen.getByTestId('tool-line-d1').querySelector('.forge-shimmer')).toBeNull();
    // 首帧已在的行不重播入场
    expect(screen.getByTestId('tool-line-d1').closest('.forge-stream-in')).toBeNull();

    rerender(<ActivitySegment blocks={[done, running]} indices={[0, 1]} streaming />);
    expect(screen.getByTestId('activity-segment').textContent).toContain('Exploring');
    expect(screen.getByTestId('activity-segment').querySelector('.forge-shimmer')).not.toBeNull();
    const line = screen.getByTestId('tool-line-r1');
    expect(line.textContent).toBe('Reading a.ts');
    expect(line.querySelector('.forge-shimmer')).not.toBeNull();
    expect(line.closest('.forge-stream-in')).not.toBeNull();

    // 消息已收束(非流式):running 残留也不扫
    rerender(<ActivitySegment blocks={[done, running]} indices={[0, 1]} />);
    expect(document.querySelector('.forge-shimmer')).toBeNull();
  });

  it('运行态扫光:命令块在跑时命令文字扫光,收尾即停', () => {
    const cmd = toolBlock({
      toolCallId: 'sh1', name: 'shell', toolKind: 'command', mcp: null, ok: undefined,
      args: JSON.stringify({ command: 'cargo test' }),
    }) as Extract<ChatBlock, { kind: 'tool' }>;
    const { rerender } = render(<CommandBlock block={cmd} live />);
    const head = screen.getByTestId('command-block-sh1');
    expect(head.querySelector('.forge-shimmer')?.textContent).toBe('cargo test');
    rerender(<CommandBlock block={{ ...cmd, ok: true, status: 'done' }} />);
    expect(screen.getByTestId('command-block-sh1').querySelector('.forge-shimmer')).toBeNull();
  });
});

describe('<SubagentRow />', () => {
  it('双行摘要 + 点击开 overlay(组件就绪,事件面不触发)', () => {
    const openSubagent = vi.fn();
    useChatStore.setState({ openSubagent });
    render(
      <SubagentRow
        block={{ kind: 'subagent', id: 's1', label: '探索后端', status: 'done', summary: '最终摘要', work: [] }}
      />,
    );
    expect(screen.getByText('探索后端')).toBeInTheDocument();
    expect(screen.getByText('最终摘要')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('subagent-row'));
    expect(openSubagent).toHaveBeenCalledWith('s1');
  });

  it('D-036 Stop:后台子代理只中止自己的 run;同步子代理仍中止父轮', () => {
    const cancelSubagent = vi.fn();
    const cancelRun = vi.fn();
    useChatStore.setState({ cancelSubagent, cancelRun, openSubagent: vi.fn() });
    const { rerender } = render(
      <SubagentRow
        block={{
          kind: 'subagent', id: 'run_bg', label: '摆放僵尸', status: 'running',
          detachedRunId: 'run_bg', work: [],
        }}
      />,
    );
    fireEvent.click(screen.getByTestId('subagent-stop'));
    expect(cancelSubagent).toHaveBeenCalledWith('run_bg');
    expect(cancelRun).not.toHaveBeenCalled();
    // 无 detachedRunId(同步 task 子代理)→ 维持中止父轮的原语义。
    rerender(
      <SubagentRow block={{ kind: 'subagent', id: 'c1', label: '探索', status: 'running', work: [] }} />,
    );
    fireEvent.click(screen.getByTestId('subagent-stop'));
    expect(cancelRun).toHaveBeenCalled();
  });

  it('状态粒子随 status 切换(running 动态 / done 静态 / error 红)', () => {
    const block = (status: 'running' | 'done' | 'error'): Extract<ChatBlock, { kind: 'subagent' }> => ({
      kind: 'subagent',
      id: 's2',
      label: '探索后端',
      status,
      work: [],
    });
    const { rerender } = render(<SubagentRow block={block('running')} />);
    const particles = screen.getByTestId('subagent-particles');
    expect(particles).toHaveAttribute('data-state', 'running');
    rerender(<SubagentRow block={block('done')} />);
    expect(screen.getByTestId('subagent-particles')).toHaveAttribute('data-state', 'done');
    rerender(<SubagentRow block={block('error')} />);
    expect(screen.getByTestId('subagent-particles')).toHaveAttribute('data-state', 'error');
  });

  it('D-047 运行中末行(实时进展)扫光,完成后回浅灰', () => {
    const block = (status: 'running' | 'done'): Extract<ChatBlock, { kind: 'subagent' }> => ({
      kind: 'subagent', id: 's3', label: '探索后端', status, summary: status === 'done' ? '最终摘要' : undefined, work: [],
    });
    const { rerender } = render(<SubagentRow block={block('running')} />);
    expect(screen.getByText('Planning next moves')).toHaveClass('forge-shimmer');
    expect(screen.getByText('探索后端')).not.toHaveClass('forge-shimmer');
    rerender(<SubagentRow block={block('done')} />);
    expect(screen.getByText('最终摘要')).not.toHaveClass('forge-shimmer');
    expect(screen.getByText('最终摘要')).toHaveClass('text-fg-4');
  });
});

describe('<SubagentOverlay /> WORK 折叠', () => {
  /** 构造带子代理块的助手消息(overlay 数据源:chatStore.messages + subagentOverlayId)。 */
  function assistantWithSub(work: ChatBlock[]): ChatMsg {
    return {
      id: 'm1',
      role: 'assistant',
      text: '',
      blocks: [
        { kind: 'subagent', id: 's1', label: '探索代码库', status: 'done', summary: '完成', work },
      ],
      status: 'completed',
      time: '10:25',
    };
  }

  const workWithReport: ChatBlock[] = [
    toolBlock({ toolCallId: 't1', name: 'grep', args: '{"pattern":"foo"}', mcp: null }),
    toolBlock({ toolCallId: 't2', name: 'read_file', args: '{"path":"a.rs"}', mcp: null }),
    toolBlock({ toolCallId: 't3', name: 'read_file', args: '{"path":"b.rs"}', mcp: null }),
    { kind: 'text', text: '查完了，结果如下', final: true },
  ];

  it('黑色汇报输出出现:上方灰色过程折叠为统计行,点击展开回看', () => {
    useChatStore.setState({
      messages: [assistantWithSub(workWithReport)],
      subagentOverlayId: 's1',
    });
    render(<SubagentOverlay />);
    // 灰色过程行折叠为一行统计(探索 2 文件 + 1 次搜索)
    expect(screen.getByTestId('subagent-proc-toggle')).toHaveTextContent(
      'Explored 2 files, 1 search',
    );
    // 黑色汇报仍全量可见
    expect(screen.getByText('查完了，结果如下')).toBeInTheDocument();
    // 过程行默认收起
    expect(screen.queryByTestId('tool-line-t1')).not.toBeInTheDocument();
    // 点击展开回看过程(内层 activity 段默认仍聚合,再点开见逐工具行)
    fireEvent.click(screen.getByTestId('subagent-proc-toggle'));
    expect(screen.getByTestId('activity-segment')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('activity-segment'));
    expect(screen.getByTestId('tool-line-t1')).toBeInTheDocument();
  });

  it('无汇报输出(运行中):过程行原样渲染不折叠', () => {
    useChatStore.setState({
      messages: [assistantWithSub(workWithReport.slice(0, 3))],
      subagentOverlayId: 's1',
    });
    render(<SubagentOverlay />);
    expect(screen.queryByTestId('subagent-proc-toggle')).not.toBeInTheDocument();
    expect(screen.getByTestId('activity-segment')).toBeInTheDocument();
  });

  it('头部:状态粒子 + 标题 + 语义徽标;Esc 与关闭钮都能收起', () => {
    useChatStore.setState({
      messages: [assistantWithSub([])],
      subagentOverlayId: 's1',
    });
    render(<SubagentOverlay />);
    expect(screen.getByRole('dialog', { name: '子代理:探索代码库' })).toBeInTheDocument();
    expect(screen.getByTestId('subagent-particles')).toHaveAttribute('data-state', 'done');
    expect(screen.getByTestId('subagent-overlay-status')).toHaveTextContent('已完成');

    fireEvent.keyDown(window, { key: 'Escape' });
    expect(useChatStore.getState().subagentOverlayId).toBeNull();
    expect(screen.queryByTestId('subagent-overlay')).not.toBeInTheDocument();

    act(() => useChatStore.setState({ subagentOverlayId: 's1' }));
    fireEvent.click(screen.getByTestId('subagent-overlay-close'));
    expect(useChatStore.getState().subagentOverlayId).toBeNull();
  });

  it('PROMPT 区渲染提示词;运行中徽标与摘要占位跟随状态', () => {
    const msg = assistantWithSub([]);
    msg.blocks = [
      {
        kind: 'subagent',
        id: 's1',
        label: '探索代码库',
        status: 'running',
        prompt: '修复 battle_1_1 场景中所有无效的 Sprite 引用',
        work: [],
      },
    ];
    useChatStore.setState({ messages: [msg], subagentOverlayId: 's1' });
    render(<SubagentOverlay />);
    expect(screen.getByText('修复 battle_1_1 场景中所有无效的 Sprite 引用')).toBeInTheDocument();
    expect(screen.getByTestId('subagent-overlay-status')).toHaveTextContent('运行中');
    expect(screen.getByText('子 agent 正在工作，完成后会在这里显示摘要。')).toBeInTheDocument();
    expect(screen.queryByTestId('subagent-work')).not.toBeInTheDocument();
  });
});

describe('<MarkdownFlat />', () => {
  it('标题/引用/列表/代码块渲染:标记符号不外露', () => {
    render(<MarkdownFlat text={'# 标题\n> 引\n- 项\n```\ncode\n```'} />);
    const root = screen.getByTestId('markdown-flat');
    expect(root.textContent).toContain('标题');
    expect(root.textContent).not.toContain('# ');
    expect(root.textContent).not.toContain('> ');
    expect(root.textContent).not.toContain('- 项');
    expect(root.textContent).toContain('•');
    expect(screen.getByTestId('md-code').textContent).toContain('code');
  });

  it('列表里的加粗不再出现「• -」双符号;链接按类型渲染', () => {
    render(
      <MarkdownFlat
        text={'- **任务 ID**: b5ef\n- 文档 [官网](https://x.dev) 与 [脚本](src/run.ts)'}
      />,
    );
    const items = screen.getAllByTestId('md-list-item');
    expect(items[0].textContent).toBe('•任务 ID: b5ef');
    const links = screen.getAllByTestId('md-link');
    expect(links[0].tagName).toBe('A');
    expect(links[0]).toHaveAttribute('href', 'https://x.dev');
    expect(links[0]).toHaveAttribute('target', '_blank');
    expect(links[1].tagName).toBe('BUTTON');
    fireEvent.click(links[1]);
    expect(useWorkbenchStore.getState().activeTabId).toBe('file:src/run.ts');
  });

  it('表格整表渲染;代码块带语言标签与复制钮', () => {
    render(<MarkdownFlat text={'| a | b |\n|---|---|\n| 1 | 2 |\n\n```ts\nconst x = 1\n```'} />);
    const table = screen.getByTestId('md-table');
    expect(table.querySelectorAll('th')).toHaveLength(2);
    expect(table.querySelectorAll('td')).toHaveLength(2);
    expect(screen.getByTestId('md-code')).toHaveTextContent('ts');
    expect(screen.getByTestId('md-code-copy')).toBeInTheDocument();
  });

  it('strong 档:正文加粗,行内代码与代码块回正常字重', () => {
    render(<MarkdownFlat strong text={'用 `npm run dev` 启动\n```\nx\n```'} />);
    const root = screen.getByTestId('markdown-flat');
    expect(root.className).toContain('font-bold');
    expect(root.querySelector('code')?.className).toContain('font-normal');
    expect(screen.getByTestId('md-code').className).toContain('font-normal');
  });
});

describe('消息复制', () => {
  it('助手回复只复制正文(不含工具过程),用户消息复制原文', async () => {
    const writeText = vi.fn(async () => {});
    vi.stubGlobal('navigator', { ...globalThis.navigator, clipboard: { writeText } });
    const msg: ChatMsg = {
      id: 'a-copy',
      role: 'assistant',
      text: '',
      status: 'completed',
      time: '',
      blocks: [
        { kind: 'text', text: '先看一下', final: false },
        toolBlock(),
        { kind: 'text', text: '改好了', final: true },
      ],
    };
    const { unmount } = render(<AssistantMessage msg={msg} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId('assistant-copy'));
    });
    expect(writeText).toHaveBeenLastCalledWith('先看一下\n\n改好了');
    unmount();
    render(<UserMessageCard msg={userMsg()} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId('user-copy'));
    });
    expect(writeText).toHaveBeenLastCalledWith('给场景加碰撞体');
    vi.unstubAllGlobals();
  });

  it('流式中的助手消息不给复制钮', () => {
    const msg: ChatMsg = {
      id: 'a-stream',
      role: 'assistant',
      text: '',
      status: 'streaming',
      time: '',
      blocks: [{ kind: 'text', text: '写到一半', final: false }],
    };
    render(<AssistantMessage msg={msg} />);
    expect(screen.queryByTestId('assistant-copy')).not.toBeInTheDocument();
  });
});

describe('<MessageList /> 空态', () => {
  it('无会话/有会话无消息双空态', () => {
    useSessionStore.setState({ activeSessionId: null });
    const { rerender } = render(<MessageList />);
    expect(screen.getByText('选择左侧会话或点击 New Agent')).toBeInTheDocument();
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    useChatStore.setState({ messages: [], hydrating: false });
    rerender(<MessageList />);
    expect(
      screen.getByText('尚无任何消息。选择左侧会话后发送，或先点击「新建」创建会话。'),
    ).toBeInTheDocument();
  });

  it('消息渲染:user 卡 + assistant 卡分流', () => {
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    useChatStore.setState({
      messages: [
        userMsg(),
        { id: 'a1', role: 'assistant', text: '', status: 'completed', time: '', blocks: [{ kind: 'text', text: '答', final: true }] },
      ],
    });
    render(<MessageList />);
    expect(screen.getByTestId('user-message-card')).toBeInTheDocument();
    expect(screen.getByTestId('assistant-message')).toBeInTheDocument();
  });

  /// D-047:发出后、agent.started 建出真卡之前,用户卡下面先挂占位卡(同款头 + Planning next moves)。
  it('开轮前占位卡:末条是用户卡时挂出;真卡出现即被接替', () => {
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    useChatStore.setState({ messages: [userMsg({ id: 'local-1', runId: null })], pendingTurnSince: Date.now() });
    const { rerender } = render(<MessageList />);
    const pending = screen.getByTestId('assistant-pending');
    expect(within(pending).getByTestId('assistant-avatar')).toBeInTheDocument();
    expect(within(pending).getByTestId('turn-status')).toHaveAttribute('data-state', 'planning');
    expect(screen.queryByTestId('assistant-message')).not.toBeInTheDocument();

    act(() => {
      useChatStore.setState({
        messages: [
          userMsg({ id: 'local-1', runId: null }),
          { id: 'a-new', role: 'assistant', text: '', status: 'streaming', time: '10:30', runId: 'run_9', blocks: [] },
        ],
      });
    });
    rerender(<MessageList />);
    expect(screen.queryByTestId('assistant-pending')).not.toBeInTheDocument();
    expect(within(screen.getByTestId('assistant-message')).getByTestId('turn-status')).toBeInTheDocument();
  });
});
