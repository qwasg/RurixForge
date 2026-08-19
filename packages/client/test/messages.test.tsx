import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import UserMessageCard from '@/components/chat/UserMessageCard';
import AssistantMessage from '@/components/chat/AssistantMessage';
import ActivitySegment from '@/components/chat/ActivitySegment';
import SubagentRow from '@/components/chat/SubagentRow';
import MarkdownFlat from '@/components/chat/MarkdownFlat';
import MessageList from '@/components/chat/MessageList';
import type { ChatBlock } from '@/lib/timeline';

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

describe('<UserMessageCard />', () => {
  it('通栏卡:HH:MM 头 + 正文 + mode chip', () => {
    render(<UserMessageCard msg={userMsg()} />);
    expect(screen.getByText('10:24')).toBeInTheDocument();
    expect(screen.getByText('给场景加碰撞体')).toBeInTheDocument();
    expect(screen.getByText('Agent')).toBeInTheDocument(); // build → Agent
  });

  it('点击进编辑 → 改文本 → 重发 = editAndResend(id, newText);取消回退', () => {
    const editAndResend = vi.fn();
    useChatStore.setState({ editAndResend });
    render(<UserMessageCard msg={userMsg()} />);
    fireEvent.click(screen.getByTestId('user-message-text'));
    const input = screen.getByTestId('user-edit-input');
    expect(input).toHaveValue('给场景加碰撞体');
    expect(screen.getByText('编辑后重新发送，将回退此后的对话')).toBeInTheDocument();
    fireEvent.change(input, { target: { value: '改成加两个' } });
    fireEvent.click(screen.getByTestId('user-edit-resend'));
    expect(editAndResend).toHaveBeenCalledWith('evt_u1', '改成加两个');
    // 再次进入后取消
    fireEvent.click(screen.getByTestId('user-message-text'));
    fireEvent.click(screen.getByTestId('user-edit-cancel'));
    expect(screen.queryByTestId('user-edit-input')).not.toBeInTheDocument();
  });

  it('运行中禁止编辑(toast 提示)', () => {
    useChatStore.setState({ activeRunId: 'run_x' });
    render(<UserMessageCard msg={userMsg()} />);
    fireEvent.click(screen.getByTestId('user-message-text'));
    expect(screen.queryByTestId('user-edit-input')).not.toBeInTheDocument();
    expect(useToastStore.getState().items.some((t) => t.title.includes('运行中'))).toBe(true);
  });
});

describe('<AssistantMessage />', () => {
  it('头:铸方块 + Agent + 模型 label + 时间;最终 text 走 markdown 不折叠', () => {
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
    expect(screen.getByTestId('assistant-avatar')).toHaveTextContent('铸');
    expect(screen.getByText('Agent')).toBeInTheDocument();
    expect(screen.getByTestId('assistant-model')).toHaveTextContent('mock');
    expect(screen.getByText('10:25')).toBeInTheDocument();
    // 最终回答完整 markdown(标题同粗同号,原始标记保留,参考同)
    expect(screen.getByTestId('markdown-flat').textContent).toContain('# 完成');
    // 工具段:单行汇总不展开
    expect(screen.getByTestId('activity-segment').textContent).toContain('编辑 1 个文件');
  });

  it('中间 text 块内联(text_2);最终块与其区分', () => {
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
  });

  it('streaming:状态点脉冲 + 末挂 stream-caret;failed 显示错误行', () => {
    const streaming: ChatMsg = {
      id: 'a3', role: 'assistant', text: '', status: 'streaming', time: '',
      blocks: [toolBlock({ ok: undefined, durationMs: undefined })],
    };
    const { rerender } = render(<AssistantMessage msg={streaming} />);
    expect(screen.getByTestId('stream-caret')).toBeInTheDocument();
    rerender(
      <AssistantMessage
        msg={{ ...streaming, status: 'failed', error: '模板未命中', blocks: streaming.blocks }}
      />,
    );
    expect(screen.queryByTestId('stream-caret')).not.toBeInTheDocument();
    expect(screen.getByTestId('assistant-error')).toHaveTextContent('模板未命中');
  });

  it('reasoning 折叠行「思考 · 摘录 · N 字」展开全文(组件就绪,事件面不触发)', () => {
    const msg: ChatMsg = {
      id: 'a4', role: 'assistant', text: '', status: 'completed', time: '',
      blocks: [{ kind: 'reasoning', text: '先分析问题\n再动手' }],
    };
    render(<AssistantMessage msg={msg} />);
    const line = screen.getByTestId('reasoning-line');
    expect(line.textContent).toContain('思考 · 先分析问题 再动手 · 9 字');
    fireEvent.click(line);
    // 展开后细节区出现全文(外层包装 div 与内层同 textContent,取 all 断言非空)
    expect(
      screen.getAllByText((_, el) => el?.textContent === '先分析问题\n再动手').length,
    ).toBeGreaterThanOrEqual(1);
  });
});

describe('<ActivitySegment />', () => {
  it('汇总行 +N/-N + 失败计数 + 运行中短语;展开见工具行;同类并组', () => {
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
    expect(seg.textContent).toContain('编辑 2 个文件');
    expect(seg.textContent).toContain('+1');
    fireEvent.click(seg);
    expect(screen.getByTestId('tool-line-c1')).toBeInTheDocument();
    expect(screen.getByTestId('tool-line-c2')).toBeInTheDocument();
    // 失败 + 运行中
    const blocks2: ChatBlock[] = [
      toolBlock({ toolCallId: 'c3', ok: false, error: '炸了' }),
      toolBlock({ toolCallId: 'c4', ok: undefined, durationMs: undefined }),
    ];
    render(<ActivitySegment blocks={blocks2} indices={[0, 1]} />);
    const segs = screen.getAllByTestId('activity-segment');
    const seg2 = segs[segs.length - 1];
    expect(seg2.textContent).toContain('· 1 失败');
    expect(seg2.textContent).toContain('正在创建实体…');
  });

  it('工具行展开细节:args mono + 完成 durationMs', () => {
    const blocks: ChatBlock[] = [toolBlock({ toolCallId: 'c9', ok: true, durationMs: 34 })];
    render(<ActivitySegment blocks={blocks} indices={[0]} />);
    fireEvent.click(screen.getByTestId('activity-segment'));
    fireEvent.click(screen.getByTestId('tool-line-c9'));
    expect(screen.getByTestId('tool-detail-args').textContent).toContain('"name": "e1"');
    expect(screen.getByText('完成 · 34ms')).toBeInTheDocument();
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
});

describe('<MarkdownFlat />', () => {
  it('标题/引用/列表/代码块渲染', () => {
    render(<MarkdownFlat text={'# 标题\n> 引\n- 项\n```\ncode\n```'} />);
    const root = screen.getByTestId('markdown-flat');
    expect(root.textContent).toContain('# 标题');
    expect(root.textContent).toContain('•');
    expect(screen.getByTestId('md-code').textContent).toContain('code');
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
});
