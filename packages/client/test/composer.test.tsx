import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import Composer from '@/components/chat/Composer';
import { mockForgeBackend } from './forgeMock';

/** F7 wave.4 Composer 全量测试(TodoStrip/五模式/技能 chips/模型菜单/发送-abort 状态机/Enter 语义)。 */

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();

beforeEach(() => {
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useSessionStore.setState({ activeSessionId: 'sess_1' });
  useChatStore.setState({
    models: [
      { id: 'deepseek-chat', label: 'deepseek-chat', provider: 'deepseek', availability: 'needs-key' },
      { id: 'mock', label: 'Mock provider', provider: 'mock', availability: 'available' },
    ],
    defaultModelId: 'deepseek-chat',
    selectedModelId: 'mock',
  });
  vi.stubGlobal(
    'fetch',
    mockForgeBackend({}, {
      '/api/forge/skills/list': {
        skills: [
          { name: 'scene-greybox', description: '灰盒搭建', enabled: true },
          { name: 'perf-budget-check', description: '', enabled: true },
        ],
      },
    }),
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function type(text: string) {
  fireEvent.change(screen.getByTestId('composer-input'), { target: { value: text } });
}

describe('<Composer /> 发送状态机', () => {
  it('空文本=禁用;有文本无会话=灰禁用+warn 胶囊;可发送=accent 发送', () => {
    render(<Composer />);
    expect(screen.getByTestId('composer-send')).toBeDisabled();
    // 有文本无会话
    act(() => useSessionStore.setState({ activeSessionId: null }));
    type('你好');
    expect(screen.getByTestId('composer-send')).toBeDisabled();
    expect(screen.getByTestId('composer-warn-no-session')).toHaveTextContent('先选择会话');
    // 有会话 → 可发送
    act(() => useSessionStore.setState({ activeSessionId: 'sess_1' }));
    expect(screen.getByTestId('composer-send')).toBeEnabled();
  });

  it('发送调用 sendMessage(text, mode) 并清空;Enter 发送 / Shift+Enter 不发送', () => {
    const sendMessage = vi.fn();
    useChatStore.setState({ sendMessage });
    render(<Composer />);
    type('说一句你好');
    fireEvent.click(screen.getByTestId('composer-send'));
    expect(sendMessage).toHaveBeenCalledWith('说一句你好', 'build');
    expect(screen.getByTestId('composer-input')).toHaveValue('');
    // Enter 发送
    type('第二条');
    fireEvent.keyDown(screen.getByTestId('composer-input'), { key: 'Enter' });
    expect(sendMessage).toHaveBeenCalledWith('第二条', 'build');
    // Shift+Enter 不发送
    sendMessage.mockClear();
    type('第三');
    fireEvent.keyDown(screen.getByTestId('composer-input'), { key: 'Enter', shiftKey: true });
    expect(sendMessage).not.toHaveBeenCalled();
  });

  it('running → danger 中止钮 → cancelRun', () => {
    const cancelRun = vi.fn();
    useChatStore.setState({ activeRunId: 'run_1', cancelRun });
    render(<Composer />);
    expect(screen.queryByTestId('composer-send')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('composer-abort'));
    expect(cancelRun).toHaveBeenCalled();
  });
});

describe('<Composer /> 模式与技能', () => {
  it('+ 菜单五模式;非 build 出 chip;x 复位', () => {
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-add'));
    for (const id of ['build', 'plan', 'debug', 'multitask', 'ask']) {
      expect(screen.getByTestId(`mode-item-${id}`)).toBeInTheDocument();
    }
    fireEvent.click(screen.getByTestId('mode-item-multitask'));
    expect(screen.getByTestId('composer-mode-chip')).toHaveTextContent('Multitask');
    const sendMessage = vi.fn();
    useChatStore.setState({ sendMessage });
    type('加碰撞体');
    fireEvent.click(screen.getByTestId('composer-send'));
    expect(sendMessage).toHaveBeenCalledWith('加碰撞体', 'multitask');
    fireEvent.click(screen.getByTestId('composer-mode-reset'));
    expect(screen.queryByTestId('composer-mode-chip')).not.toBeInTheDocument();
  });

  it('技能菜单:列表双行/选中 check + chips;发送前缀 Use skills 并清空', async () => {
    const sendMessage = vi.fn();
    useChatStore.setState({ sendMessage });
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-skills'));
    expect(await screen.findByTestId('skill-item-scene-greybox')).toBeInTheDocument();
    expect(screen.getByText('灰盒搭建')).toBeInTheDocument();
    expect(screen.getByText('（无描述）')).toBeInTheDocument(); // 空 description 占位
    fireEvent.click(screen.getByTestId('skill-item-scene-greybox'));
    fireEvent.click(screen.getByTestId('skill-item-perf-budget-check'));
    expect(screen.getByTestId('skill-chip-scene-greybox')).toBeInTheDocument();
    // 可 x 移除
    fireEvent.click(screen.getByLabelText('移除技能 perf-budget-check'));
    expect(screen.queryByTestId('skill-chip-perf-budget-check')).not.toBeInTheDocument();
    type('整理场景');
    fireEvent.click(screen.getByTestId('composer-send'));
    expect(sendMessage).toHaveBeenCalledWith('Use skills: scene-greybox.\n\n整理场景', 'build');
    expect(screen.queryByTestId('skill-chip-scene-greybox')).not.toBeInTheDocument();
  });

  it('模型菜单:label+provider 双行;needs-key 禁用+title;选中 PATCH(pickModel)', async () => {
    const pickModel = vi.fn();
    useChatStore.setState({ pickModel });
    render(<Composer />);
    // chip 显示选中模型 label
    expect(screen.getByTestId('composer-model')).toHaveTextContent('Mock provider');
    fireEvent.click(screen.getByTestId('composer-model'));
    const ds = await screen.findByTestId('model-item-deepseek-chat');
    expect(ds).toBeDisabled();
    expect(ds).toHaveAttribute('title', '未配置 Key');
    expect(screen.getByText('deepseek')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('model-item-mock'));
    expect(pickModel).toHaveBeenCalledWith('mock');
  });

  it('F8 wave.1:联网开关=诚实禁用态(disabled + tooltip,不可开关)', () => {
    render(<Composer />);
    const ws = screen.getByTestId('composer-websearch');
    expect(ws).toBeDisabled();
    expect(ws).toHaveAttribute('title', '联网搜索后端未接入');
  });
});

describe('<Composer /> TodoStrip', () => {
  it('todos 非空:TODO + done/total + 进度条;展开排序(running 先)完成行划线', () => {
    useChatStore.setState({
      todos: [
        { id: 't1', title: '排队项', status: 'queued' },
        { id: 't2', title: '完成项', status: 'completed' },
        { id: 't3', title: '进行项', status: 'running' },
      ],
    });
    render(<Composer />);
    expect(screen.getByTestId('todo-strip')).toHaveTextContent('TODO');
    expect(screen.getByText('1/3')).toBeInTheDocument();
    expect(screen.getByTestId('todo-progress').style.width).toBe('33%');
    // 默认折叠无行
    expect(screen.queryByTestId('todo-row-t1')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('todo-strip-toggle'));
    const rows = screen.getAllByTestId(/^todo-row-/);
    expect(rows[0]).toHaveTextContent('进行项'); // running 排前
    expect(rows[2]).toHaveTextContent('完成项');
    expect(rows[2].querySelector('.line-through')).not.toBeNull();
  });

  it('todos 为空不渲染 strip', () => {
    render(<Composer />);
    expect(screen.queryByTestId('todo-strip')).not.toBeInTheDocument();
  });
});
