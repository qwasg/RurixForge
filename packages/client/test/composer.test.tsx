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
  it('+ 菜单含 AgentKind 三节与六模式;非 build 时 + 钮内出模式标签;x 复位', () => {
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-add'));
    for (const id of ['coding', 'general', 'document']) {
      expect(screen.getByTestId(`kind-item-${id}`)).toBeInTheDocument();
    }
    for (const id of ['build', 'plan', 'team', 'debug', 'multitask', 'ask']) {
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

  it('技能菜单:列表双行/选中 check + chips;发送走结构化 skills 形参并清空', async () => {
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
    // F11:技能名不再拼进正文,改作 sendMessage 第三形参下发(ask:execute skills[])
    expect(sendMessage).toHaveBeenCalledWith('整理场景', 'build', ['scene-greybox']);
    expect(screen.queryByTestId('skill-chip-scene-greybox')).not.toBeInTheDocument();
  });

  it('模型子菜单:label+provider 双行;needs-key 禁用+title;选中 PATCH(pickModel)', async () => {
    const pickModel = vi.fn();
    useChatStore.setState({ pickModel });
    render(<Composer />);
    // chip 显示选中模型 label
    expect(screen.getByTestId('composer-model')).toHaveTextContent('Mock provider');
    fireEvent.click(screen.getByTestId('composer-model'));
    fireEvent.click(screen.getByTestId('spec-row-model'));
    const ds = await screen.findByTestId('model-item-deepseek-chat');
    expect(ds).toBeDisabled();
    expect(ds).toHaveAttribute('title', '未配置 Key');
    expect(ds).toHaveTextContent('deepseek · 未配置 Key');
    fireEvent.click(screen.getByTestId('model-item-mock'));
    expect(pickModel).toHaveBeenCalledWith('mock');
  });

  it('技能/模型钮落在胶囊下方工具行;联网搜索钮已移除', () => {
    render(<Composer />);
    expect(screen.queryByTestId('composer-websearch')).not.toBeInTheDocument();
    const tools = screen.getByTestId('composer-tools');
    expect(tools).toContainElement(screen.getByTestId('composer-skills'));
    expect(tools).toContainElement(screen.getByTestId('composer-model'));
    expect(screen.getByTestId('composer-capsule')).not.toContainElement(
      screen.getByTestId('composer-model'),
    );
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

describe('<Composer /> 单行胶囊', () => {
  const inputShell = () => screen.getByTestId('composer-input').parentElement as HTMLElement;

  it('空文本:输入压到 26px,胶囊走 rounded-full 且只含 + / 输入 / 发送', () => {
    render(<Composer />);
    expect(inputShell().style.height).toBe('26px');
    expect(screen.getByTestId('composer')).toHaveAttribute('data-capsule', '1');
    const pill = screen.getByTestId('composer-capsule');
    expect(pill.className).toContain('rounded-full');
    expect(pill).toContainElement(screen.getByTestId('composer-add'));
    expect(pill).toContainElement(screen.getByTestId('composer-input'));
    expect(pill).toContainElement(screen.getByTestId('composer-send'));
  });

  it('单行文本保持胶囊;换行后长高并退回 rounded-2xl', () => {
    render(<Composer />);
    type('单行');
    expect(inputShell().style.height).toBe('26px');
    expect(screen.getByTestId('composer')).toHaveAttribute('data-capsule', '1');
    type('第一行\n第二行');
    expect(inputShell().style.height).toBe('46px');
    expect(screen.getByTestId('composer')).not.toHaveAttribute('data-capsule');
    expect(screen.getByTestId('composer-capsule').className).toContain('rounded-2xl');
  });

  it('TodoStrip 移到胶囊外,不再撑破胶囊形状', () => {
    useChatStore.setState({ todos: [{ id: 't1', title: '排队项', status: 'queued' }] });
    render(<Composer />);
    expect(screen.getByTestId('composer')).toHaveAttribute('data-capsule', '1');
    expect(screen.getByTestId('composer-capsule')).not.toContainElement(
      screen.getByTestId('todo-strip'),
    );
  });

  it('模式 chip 与 [+] 钮融合:非 build 时 + 钮拉宽显示模式并带 × 复位,不再另占 chip 行', () => {
    render(<Composer />);
    // build:纯 + 圆钮,无模式标签/复位钮
    const pill = screen.getByTestId('composer-mode-pill');
    expect(pill).toHaveAttribute('data-mode', 'build');
    expect(screen.queryByTestId('composer-mode-chip')).not.toBeInTheDocument();
    expect(screen.queryByTestId('composer-mode-reset')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('composer-add'));
    fireEvent.click(screen.getByTestId('mode-item-plan'));
    // plan:模式标签落在 + 钮内部,× 与其同壳;胶囊保持单行,上方无 chip 行
    expect(pill).toHaveAttribute('data-mode', 'plan');
    expect(screen.getByTestId('composer-add')).toContainElement(
      screen.getByTestId('composer-mode-chip'),
    );
    expect(pill).toContainElement(screen.getByTestId('composer-mode-reset'));
    expect(screen.getByTestId('composer-capsule')).toContainElement(pill);
    expect(screen.queryByTestId('composer-chips')).not.toBeInTheDocument();
    expect(screen.getByTestId('composer')).toHaveAttribute('data-capsule', '1');
    // × 复位:回到纯 + 圆钮,+ 钮本身仍在
    fireEvent.click(screen.getByTestId('composer-mode-reset'));
    expect(pill).toHaveAttribute('data-mode', 'build');
    expect(screen.queryByTestId('composer-mode-chip')).not.toBeInTheDocument();
    expect(screen.getByTestId('composer-add')).toBeInTheDocument();
  });

  it('技能 chip 行独立于模式:只选技能时出 chip 行,只切模式时不出', () => {
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-add'));
    fireEvent.click(screen.getByTestId('mode-item-debug'));
    expect(screen.queryByTestId('composer-chips')).not.toBeInTheDocument();
    expect(screen.getByTestId('composer-mode-chip')).toHaveTextContent('Debug');
  });
});
