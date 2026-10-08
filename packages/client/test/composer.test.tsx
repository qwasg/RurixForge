import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import Composer from '@/components/chat/Composer';
import { useGoalStore } from '@/lib/goalStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useUltraPlanStore, type UltraPlanState } from '@/lib/ultraPlanStore';
import { mockForgeBackend } from './forgeMock';

/** F7 wave.4 Composer 全量测试(TodoStrip/五模式/技能 chips/模型菜单/发送-abort 状态机/Enter 语义)。 */

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialGoal = useGoalStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
const initialOverlay = useOverlayStore.getState();
const initialSettings = useSettingsStore.getState();

beforeEach(() => {
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useGoalStore.setState(initialGoal, true);
  useGoalStore.getState().reset();
  useWorkbenchStore.setState(initialWorkbench, true);
  useOverlayStore.setState(initialOverlay, true);
  useSettingsStore.setState(initialSettings, true);
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
  it('Codex 检查未完成时不发送首条消息，切换完成后以 Codex 创建会话', async () => {
    let releaseStatus!: () => void;
    const pendingStatus = new Promise<void>((resolve) => {
      releaseStatus = resolve;
    });
    const backend = mockForgeBackend({}, {
      '/api/forge/codex/status': {
        installed: true, running: true, config: {},
        account: { authMode: 'chatgpt' }, install: { running: false, log: '' },
      },
    });
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: { body?: string }) => {
      if (String(url) === '/api/forge/codex/status') await pendingStatus;
      return backend(url, init);
    }));
    const createSession = vi.fn(async () => null);
    const sendMessage = vi.fn(async () => undefined);
    useSessionStore.setState({ activeSessionId: null, create: createSession });
    useChatStore.setState({
      sendMessage,
      selectedModelId: null,
      models: [{ id: 'codex:gpt-5', label: 'gpt-5', provider: 'codex', availability: 'available' }],
      ensureModels: vi.fn(async () => undefined),
    });
    render(<Composer variant="home" />);
    type('你好');
    fireEvent.click(screen.getByTestId('agent-engine-codex'));

    expect(screen.getByTestId('composer-send')).toBeDisabled();
    fireEvent.click(screen.getByTestId('composer-send'));
    fireEvent.keyDown(screen.getByTestId('composer-input'), { key: 'Enter' });
    expect(createSession).not.toHaveBeenCalled();
    expect(sendMessage).not.toHaveBeenCalled();
    expect(screen.getByTestId('composer-input')).toHaveValue('你好');

    await act(async () => {
      releaseStatus();
      await pendingStatus;
    });
    await waitFor(() => expect(screen.getByTestId('agent-engine-codex')).toHaveAttribute('aria-pressed', 'true'));
    await waitFor(() => expect(screen.getByTestId('composer-send')).toBeEnabled());
    fireEvent.click(screen.getByTestId('composer-send'));
    await waitFor(() => expect(createSession).toHaveBeenCalledWith(undefined, expect.objectContaining({ agentEngine: 'codex' })));
  });

  it('POST 已提交但 SSE started 未到时锁定引擎与模型', async () => {
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    useChatStore.setState({ activeRunId: null, sendMessage: vi.fn(() => pending) });
    render(<Composer />);
    type('只回复 PONG');
    fireEvent.click(screen.getByTestId('composer-send'));

    await waitFor(() => {
      expect(screen.getByTestId('agent-engine-local')).toBeDisabled();
      expect(screen.getByTestId('agent-engine-codex')).toBeDisabled();
      expect(screen.getByTestId('composer-model')).toBeDisabled();
    });
    await act(async () => {
      release();
      await pending;
    });
    await waitFor(() => expect(screen.getByTestId('composer-model')).toBeEnabled());
  });

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
    expect(screen.getByTestId('composer-input')).toHaveValue(''); // drafts belong to their conversation
    type('当前会话的消息');
    expect(screen.getByTestId('composer-send')).toBeEnabled();
  });

  it('发送调用 sendMessage(text, mode) 并清空;Enter 发送 / Shift+Enter 不发送', async () => {
    const sendMessage = vi.fn(async () => undefined);
    useChatStore.setState({ sendMessage });
    render(<Composer />);
    type('说一句你好');
    fireEvent.click(screen.getByTestId('composer-send'));
    expect(sendMessage).toHaveBeenCalledWith('说一句你好', 'build');
    expect(screen.getByTestId('composer-input')).toHaveValue('说一句你好'); // retained until accepted
    await act(async () => Promise.resolve());
    expect(screen.getByTestId('composer-input')).toHaveValue('');
    await act(async () => Promise.resolve());
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
  it('AgentSwitcher 切 Codex；Codex 模式隐藏 team/multitask 且模型只列 codex provider', async () => {
    const setAgentEngine = vi.fn();
    useSessionStore.setState({
      sessions: [{
        id: 'sess_1', title: 'x', status: 'idle', agentKind: 'coding', agentEngine: 'local',
        selectedModelId: 'mock', thinkingEnabled: false, reasoningEffort: null,
        contextOptionId: null, webSearchEnabled: false, activeRunId: null,
        createdAt: '', updatedAt: '', pinned: false, titleManuallySet: false,
      }],
      setAgentEngine,
    });
    useChatStore.setState({
      models: [
        { id: 'mock', label: 'Mock provider', provider: 'mock', availability: 'available' },
        { id: 'codex:gpt-5', label: 'gpt-5', provider: 'codex', group: 'Codex', availability: 'available' },
      ],
    });
    vi.stubGlobal('fetch', mockForgeBackend({}, {
      '/api/forge/codex/status': {
        installed: true, managedInstalled: true, computerUseInstalled: true, npmAvailable: true,
        running: true, config: {}, account: { authMode: 'chatgpt', planType: 'Plus' }, install: { running: false, log: '' },
      },
    }));
    render(<Composer />);
    fireEvent.click(screen.getByTestId('agent-engine-codex'));
    await waitFor(() => expect(setAgentEngine).toHaveBeenCalledWith('sess_1', 'codex'));
    expect(screen.getByTitle('Plus')).toBeInTheDocument();

    act(() => {
      useSessionStore.setState((state) => ({
        sessions: state.sessions.map((session) => ({ ...session, agentEngine: 'codex' as const })),
      }));
      useChatStore.setState({ selectedModelId: 'codex:gpt-5' });
    });
    fireEvent.click(screen.getByTestId('composer-add'));
    expect(screen.getByTestId('mode-item-team')).toBeInTheDocument();
    expect(screen.queryByTestId('mode-item-multitask')).not.toBeInTheDocument();
    expect(screen.getByTestId('mode-item-plan')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('composer-add'));
    fireEvent.click(screen.getByTestId('composer-model'));
    fireEvent.click(screen.getByTestId('spec-row-model'));
    expect(screen.getByTestId('model-item-codex:gpt-5')).toBeInTheDocument();
    expect(screen.queryByTestId('model-item-mock')).not.toBeInTheDocument();
  });

  it('AgentSwitcher 在 status 认证缓存为空时复查 account，不误导已登录 CLI 用户', async () => {
    const setAgentEngine = vi.fn();
    useSessionStore.setState({
      sessions: [{
        id: 'sess_1', title: 'x', status: 'idle', agentKind: 'coding', agentEngine: 'local',
        selectedModelId: 'mock', thinkingEnabled: false, reasoningEffort: null,
        contextOptionId: null, webSearchEnabled: false, activeRunId: null,
        createdAt: '', updatedAt: '', pinned: false, titleManuallySet: false,
      }],
      setAgentEngine,
    });
    vi.stubGlobal('fetch', mockForgeBackend({}, {
      '/api/forge/codex/status': {
        installed: true, managedInstalled: false, computerUseInstalled: false, npmAvailable: true,
        running: false, config: {}, account: { authMode: null }, install: { running: false, log: '' },
      },
      '/api/forge/codex/account': { ok: true, authMode: 'chatgpt', planType: 'Plus' },
    }));
    render(<Composer />);
    fireEvent.click(screen.getByTestId('agent-engine-codex'));
    await waitFor(() => expect(setAgentEngine).toHaveBeenCalledWith('sess_1', 'codex'));
  });

  it('切换到 Codex 保持「自动」语义，不用模型列表首项覆盖默认配置', async () => {
    const session = {
      id: 'sess_1', title: 'x', status: 'idle', agentKind: 'coding', agentEngine: 'local' as const,
      selectedModelId: 'mock', thinkingEnabled: false, reasoningEffort: null,
      contextOptionId: null, webSearchEnabled: false, activeRunId: null,
      createdAt: '', updatedAt: '', pinned: false, titleManuallySet: false,
    };
    const setAgentEngine = vi.fn(async (id: string, agentEngine: 'local' | 'codex') => {
      useSessionStore.setState((state) => ({
        sessions: state.sessions.map((item) => item.id === id ? { ...item, agentEngine } : item),
      }));
    });
    const ensureModels = vi.fn(async () => undefined);
    const pickModel = vi.fn(async () => undefined);
    useSessionStore.setState({ sessions: [session], setAgentEngine });
    useChatStore.setState({
      selectedModelId: 'mock',
      models: [
        { id: 'mock', label: 'Mock', provider: 'mock', availability: 'available' },
        { id: 'codex:gpt-first', label: 'GPT First', provider: 'codex', availability: 'available' },
      ],
      ensureModels,
      pickModel,
    });
    vi.stubGlobal('fetch', mockForgeBackend({}, {
      '/api/forge/codex/status': {
        installed: true, npmAvailable: true, running: true, config: {},
        account: { authMode: 'chatgpt' }, install: { running: false, log: '' },
      },
    }));

    render(<Composer />);
    fireEvent.click(screen.getByTestId('agent-engine-codex'));
    await waitFor(() => expect(pickModel).toHaveBeenCalledWith(null));
    expect(pickModel).not.toHaveBeenCalledWith('codex:gpt-first');
  });

  it('当前已是 Codex 但未登录时，再点入口会打开可处理问题的设置页', async () => {
    const setAgentEngine = vi.fn();
    useSessionStore.setState({
      sessions: [{
        id: 'sess_1', title: 'x', status: 'idle', agentKind: 'coding', agentEngine: 'codex',
        selectedModelId: null, thinkingEnabled: false, reasoningEffort: null,
        contextOptionId: null, webSearchEnabled: false, activeRunId: null,
        createdAt: '', updatedAt: '', pinned: false, titleManuallySet: false,
      }],
      setAgentEngine,
    });
    vi.stubGlobal('fetch', mockForgeBackend({}, {
      '/api/forge/codex/status': {
        installed: true, npmAvailable: true, running: false, config: {},
        account: { authMode: null }, install: { running: false, log: '' },
      },
      '/api/forge/codex/account': { ok: true, authMode: null },
    }));

    render(<Composer />);
    fireEvent.click(screen.getByTestId('agent-engine-codex'));
    await waitFor(() => {
      expect(useOverlayStore.getState().settings).toBe(true);
      expect(useSettingsStore.getState().page).toBe('codex');
    });
    expect(setAgentEngine).not.toHaveBeenCalled();
  });

  it('AgentSwitcher 切换引擎期间换会话，不刷新或改选新会话的模型', async () => {
    let finishEngineSwitch!: () => void;
    const engineSwitch = new Promise<void>((resolve) => {
      finishEngineSwitch = resolve;
    });
    const setAgentEngine = vi.fn((id: string, agentEngine: 'local' | 'codex') => {
      useSessionStore.setState((state) => ({
        sessions: state.sessions.map((session) =>
          session.id === id ? { ...session, agentEngine } : session,
        ),
      }));
      return engineSwitch;
    });
    const ensureModels = vi.fn(async () => {
      useChatStore.setState({
        models: [
          { id: 'codex:gpt-5', label: 'gpt-5', provider: 'codex', availability: 'available' },
          { id: 'mock', label: 'Mock provider', provider: 'mock', availability: 'available' },
        ],
      });
    });
    const pickModel = vi.fn(async () => undefined);
    const session = (id: string) => ({
      id, title: id, status: 'idle', agentKind: 'coding', agentEngine: 'codex' as const,
      selectedModelId: 'codex:gpt-5', thinkingEnabled: false, reasoningEffort: null,
      contextOptionId: null, webSearchEnabled: false, activeRunId: null,
      createdAt: '', updatedAt: '', pinned: false, titleManuallySet: false,
    });
    useSessionStore.setState({
      activeSessionId: 'sess_1',
      sessions: [session('sess_1'), session('sess_2')],
      setAgentEngine,
    });
    useChatStore.setState({
      models: [
        { id: 'codex:gpt-5', label: 'gpt-5', provider: 'codex', availability: 'available' },
      ],
      selectedModelId: 'codex:gpt-5',
      ensureModels,
      pickModel,
    });

    render(<Composer />);
    fireEvent.click(screen.getByTestId('agent-engine-local'));
    await waitFor(() => expect(setAgentEngine).toHaveBeenCalledWith('sess_1', 'local'));
    act(() => useSessionStore.setState({ activeSessionId: 'sess_2' }));
    await act(async () => {
      finishEngineSwitch();
      await engineSwitch;
    });
    await waitFor(() => expect(screen.getByTestId('agent-engine-local')).toBeEnabled());

    expect(ensureModels).not.toHaveBeenCalled();
    expect(pickModel).not.toHaveBeenCalled();
  });

  it('/goal 命令走 GoalStore，不作为普通聊天消息发送', async () => {
    const setGoal = vi.fn(async () => null);
    const sendMessage = vi.fn();
    useGoalStore.setState({ setGoal, loadGoal: vi.fn(async () => undefined) });
    useChatStore.setState({ sendMessage });
    render(<Composer />);
    type('/goal 做出可玩的垂直切片');
    fireEvent.click(screen.getByTestId('composer-send'));
    await act(async () => Promise.resolve());
    expect(setGoal).toHaveBeenCalledWith('做出可玩的垂直切片');
    expect(sendMessage).not.toHaveBeenCalled();
  });

  it('/goal pause|resume|clear 分派控制动作；GoalBar 显示预算并可打开页签', async () => {
    const pauseGoal = vi.fn(async () => null);
    const resumeGoal = vi.fn(async () => null);
    const clearGoal = vi.fn(async () => true);
    const openTab = vi.fn();
    useWorkbenchStore.setState({ openTab });
    useGoalStore.setState({
      sessionId: 'sess_1',
      goal: {
        sessionId: 'sess_1', objective: '持续打磨关卡', status: 'active', engine: 'local',
        tokenBudget: 10_000, tokensUsed: 2_500, timeUsedSeconds: 65, turns: 2,
      },
      pauseGoal,
      resumeGoal,
      clearGoal,
    });
    render(<Composer />);
    expect(screen.getByTestId('goal-bar')).toHaveTextContent('持续打磨关卡');
    expect(screen.getByTestId('goal-bar')).toHaveTextContent('2,500 / 10,000 tokens');
    fireEvent.click(screen.getByTestId('goal-bar-open'));
    expect(openTab).toHaveBeenCalledWith('goal');
    for (const [command, spy] of [
      ['/goal pause', pauseGoal],
      ['/goal resume', resumeGoal],
      ['/goal clear', clearGoal],
    ] as const) {
      type(command);
      fireEvent.click(screen.getByTestId('composer-send'));
      await act(async () => Promise.resolve());
      expect(spy).toHaveBeenCalled();
    }
  });

  it('+ 菜单直接显示模式;非 build 时 + 钮内出模式标签;x 复位', () => {
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-add'));
    for (const id of ['build', 'plan', 'ultraplan', 'team', 'debug', 'multitask', 'ask']) {
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

  /** D-044:UltraPlan 是第七个模式,只对「本地引擎 + coding 代理」开放。 */
  const sessionOf = (agentKind: string, agentEngine: 'local' | 'codex') => ({
    id: 'sess_1', title: 'x', status: 'idle', agentKind, agentEngine,
    selectedModelId: 'mock', thinkingEnabled: false, reasoningEffort: null,
    contextOptionId: null, webSearchEnabled: false, activeRunId: null,
    createdAt: '', updatedAt: '', pinned: false, titleManuallySet: false,
  });

  it('UltraPlan:coding + 本地引擎下列在模式菜单里,选中后按 mode=ultraplan 发送', () => {
    useSessionStore.setState({ sessions: [sessionOf('coding', 'local')] });
    const sendMessage = vi.fn();
    useChatStore.setState({ sendMessage });
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-add'));
    // 原六个模式一个不少,外加 ultraplan
    for (const id of ['build', 'plan', 'ultraplan', 'team', 'debug', 'multitask', 'ask']) {
      expect(screen.getByTestId(`mode-item-${id}`)).toBeInTheDocument();
    }
    expect(screen.getByTestId('mode-item-ultraplan')).toHaveTextContent('UltraPlan');
    fireEvent.click(screen.getByTestId('mode-item-ultraplan'));
    expect(screen.getByTestId('composer-mode-chip')).toHaveTextContent('UltraPlan');
    type('做一个塔防小游戏');
    fireEvent.click(screen.getByTestId('composer-send'));
    expect(sendMessage).toHaveBeenCalledWith('做一个塔防小游戏', 'ultraplan');
  });

  it('UltraPlan:codex 引擎开放 UltraPlan / Team,仍不列 Multitask', () => {
    useSessionStore.setState({ sessions: [sessionOf('coding', 'codex')] });
    useChatStore.setState({
      models: [{ id: 'codex:gpt-5', label: 'gpt-5', provider: 'codex', availability: 'available' }],
      selectedModelId: 'codex:gpt-5',
    });
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-add'));
    expect(screen.getByTestId('mode-item-ultraplan')).toBeInTheDocument();
    expect(screen.getByTestId('mode-item-team')).toBeInTheDocument();
    expect(screen.queryByTestId('mode-item-multitask')).not.toBeInTheDocument();
    expect(screen.getByTestId('mode-item-plan')).toBeInTheDocument();
  });

  it.each(['general', 'document'])('UltraPlan:%s 代理下不列(只剩 Agent / Ask)', (kind) => {
    useSessionStore.setState({ sessions: [sessionOf(kind, 'local')] });
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-add'));
    expect(screen.queryByTestId('mode-item-ultraplan')).not.toBeInTheDocument();
    expect(screen.getByTestId('mode-item-build')).toBeInTheDocument();
    expect(screen.getByTestId('mode-item-ask')).toBeInTheDocument();
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

/**
 * D-044:模式是 Composer 的本地 state(不持久、不随会话)。流程停在需求 / 问卷 / Demo / 计划
 * 关口时自动选中 ultraplan;流程没了 / 走完 / 进了制作后,把**自动选上的** ultraplan 退回 Agent;
 * 用户在没有流程的会话里手动选的 ultraplan 不动。占位文案按关口说明输入会发生什么。
 */
describe('<Composer /> UltraPlan 流程联动', () => {
  const flow = (patch: Partial<UltraPlanState> = {}): UltraPlanState => ({
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
  const session = (id: string, agentKind: string, agentEngine: 'local' | 'codex') => ({
    id, title: 'x', status: 'idle', agentKind, agentEngine,
    selectedModelId: 'mock', thinkingEnabled: false, reasoningEffort: null,
    contextOptionId: null, webSearchEnabled: false, activeRunId: null,
    createdAt: '', updatedAt: '', pinned: false, titleManuallySet: false,
  });
  /** 等同快照回填:把流程挂到某个会话上(null = 该会话没有流程)。 */
  const hydrate = (state: UltraPlanState | null, sessionId = 'sess_1') =>
    act(() => useUltraPlanStore.getState().hydrate(state, sessionId));
  const modeOf = () => screen.getByTestId('composer-mode-pill').getAttribute('data-mode');
  const placeholder = () => screen.getByTestId('composer-input').getAttribute('placeholder');

  it('会话的流程停在关口:自动选中 ultraplan,发送按 mode=ultraplan;占位文案随关口变', () => {
    const sendMessage = vi.fn();
    useChatStore.setState({ sendMessage });
    useUltraPlanStore.getState().hydrate(flow(), 'sess_1');
    render(<Composer />);
    expect(modeOf()).toBe('ultraplan');
    expect(screen.getByTestId('composer-mode-chip')).toHaveTextContent('UltraPlan');
    expect(placeholder()).toBe('在此输入补充说明会重新生成问卷');
    type('敌人再多一种');
    fireEvent.click(screen.getByTestId('composer-send'));
    expect(sendMessage).toHaveBeenCalledWith('敌人再多一种', 'ultraplan');

    hydrate(flow({ stage: 'demo_review', demoIteration: 1 }));
    expect(modeOf()).toBe('ultraplan');
    expect(placeholder()).toBe('描述要修改的地方,会重建 Demo');
    hydrate(flow({ stage: 'plan_review', demoIteration: 1, planRev: 1 }));
    expect(placeholder()).toBe('描述要调整的地方,会重写计划');
    hydrate(flow({ stage: 'discovery', questionnaireRev: 0 }));
    expect(modeOf()).toBe('ultraplan');
    expect(placeholder()).toBe('补充你的设想,会据此继续梳理需求');
  });

  it('没有流程时手动选 ultraplan:占位「详细描述你想做的游戏…」;其余模式占位不变', () => {
    render(<Composer />);
    expect(modeOf()).toBe('build');
    expect(placeholder()).toBe('描述任务…');
    fireEvent.click(screen.getByTestId('composer-add'));
    fireEvent.click(screen.getByTestId('mode-item-ultraplan'));
    expect(placeholder()).toBe('详细描述你想做的游戏…');
    fireEvent.click(screen.getByTestId('composer-add'));
    fireEvent.click(screen.getByTestId('mode-item-plan'));
    expect(placeholder()).toBe('描述任务…');
  });

  it('流程到关口才出现(实时):从 Agent 自动切到 ultraplan', () => {
    render(<Composer />);
    expect(modeOf()).toBe('build');
    hydrate(flow({ stage: 'discovery', questionnaireRev: 0, phase: 'running', running: 'discovery' }));
    expect(modeOf()).toBe('ultraplan');
  });

  it('自动选上的 ultraplan:流程被重新开始 / 走完 / 进入制作后退回 Agent', () => {
    useUltraPlanStore.getState().hydrate(flow(), 'sess_1');
    render(<Composer />);
    expect(modeOf()).toBe('ultraplan');
    // 重新开始(流程清空)
    hydrate(null);
    expect(modeOf()).toBe('build');
    expect(placeholder()).toBe('描述任务…');
    expect(screen.queryByTestId('composer-mode-chip')).not.toBeInTheDocument();

    // 再来一轮 → 进入制作(ultraplan 自由文本此时会被后端 409)
    hydrate(flow({ id: 'up_b2', stage: 'plan_review', demoIteration: 1, planRev: 1 }));
    expect(modeOf()).toBe('ultraplan');
    hydrate(flow({ id: 'up_b2', stage: 'production', demoIteration: 1, planRev: 1 }));
    expect(modeOf()).toBe('build');

    // 走完
    hydrate(flow({ id: 'up_c3' }));
    expect(modeOf()).toBe('ultraplan');
    hydrate(flow({ id: 'up_c3', stage: 'done', demoIteration: 1, planRev: 1, acceptanceRound: 1 }));
    expect(modeOf()).toBe('build');
  });

  it('切到没有流程的会话:自动选上的 ultraplan 不跟过去(否则一条普通消息就新开一轮流程)', () => {
    useSessionStore.setState({
      sessions: [session('sess_1', 'coding', 'local'), session('sess_2', 'coding', 'local')],
    });
    useUltraPlanStore.getState().hydrate(flow(), 'sess_1');
    render(<Composer />);
    expect(modeOf()).toBe('ultraplan');
    // 切会话:activeSessionId 先变,store 里还挂着上一个会话的流程 → 已按「没有流程」处理
    act(() => useSessionStore.setState({ activeSessionId: 'sess_2' }));
    expect(modeOf()).toBe('build');
    hydrate(null, 'sess_2');
    expect(modeOf()).toBe('build');
    // 切回有流程的会话 → 再次自动选中
    act(() => useSessionStore.setState({ activeSessionId: 'sess_1' }));
    hydrate(flow(), 'sess_1');
    expect(modeOf()).toBe('ultraplan');
  });

  it('不与用户的手动选择较劲:没有流程的会话里手选的 ultraplan,切会话 / 回填空流程都不动', () => {
    useSessionStore.setState({
      sessions: [session('sess_1', 'coding', 'local'), session('sess_2', 'coding', 'local')],
    });
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-add'));
    fireEvent.click(screen.getByTestId('mode-item-ultraplan'));
    expect(modeOf()).toBe('ultraplan');
    hydrate(null);
    expect(modeOf()).toBe('ultraplan');
    act(() => useSessionStore.setState({ activeSessionId: 'sess_2' }));
    hydrate(null, 'sess_2');
    expect(modeOf()).toBe('ultraplan');
  });

  it('关口上用户亲手换了模式:同一关口内不再改回去;流程走到下一道关口才重新选中', () => {
    const sendMessage = vi.fn();
    useChatStore.setState({ sendMessage });
    useUltraPlanStore.getState().hydrate(flow(), 'sess_1');
    render(<Composer />);
    expect(modeOf()).toBe('ultraplan');
    fireEvent.click(screen.getByTestId('composer-mode-reset'));
    expect(modeOf()).toBe('build');
    // 同一关口内的状态变化(phase)不触发重选
    hydrate(flow({ phase: 'failed', lastError: { code: 'X', message: 'x' } }));
    expect(modeOf()).toBe('build');
    type('顺手问一句');
    fireEvent.click(screen.getByTestId('composer-send'));
    expect(sendMessage).toHaveBeenCalledWith('顺手问一句', 'build');
    hydrate(flow({ stage: 'demo_review', demoIteration: 1 }));
    expect(modeOf()).toBe('ultraplan');
    // 用户在关口上手动换成 Plan 之后流程结束:那是用户选的模式,不退回 Agent
    fireEvent.click(screen.getByTestId('composer-add'));
    fireEvent.click(screen.getByTestId('mode-item-plan'));
    hydrate(null);
    expect(modeOf()).toBe('plan');
  });

  it('Codex 关口自动选 UltraPlan;代理不是 coding 时退回 Agent', () => {
    useSessionStore.setState({ sessions: [session('sess_1', 'coding', 'codex')] });
    useUltraPlanStore.getState().hydrate(flow(), 'sess_1');
    render(<Composer />);
    expect(modeOf()).toBe('ultraplan');
    expect(screen.queryByTestId('ultraplan-bar-readonly')).not.toBeInTheDocument();
    // 切回本地引擎仍可继续同一流程
    act(() => useSessionStore.setState({ sessions: [session('sess_1', 'coding', 'local')] }));
    expect(modeOf()).toBe('ultraplan');
    // 流程中途把代理切到通用 → 退回 Agent
    act(() => useSessionStore.setState({ sessions: [session('sess_1', 'general', 'local')] }));
    expect(modeOf()).toBe('build');
  });

  it('UltraPlanBar 挂在 Composer 里(GoalBar 之后、胶囊之上);没有流程不占位', () => {
    render(<Composer />);
    expect(screen.queryByTestId('ultraplan-bar')).not.toBeInTheDocument();
    hydrate(flow({ stage: 'demo_review', demoIteration: 1 }));
    const bar = screen.getByTestId('ultraplan-bar');
    expect(screen.getByTestId('composer')).toContainElement(bar);
    expect(bar).toHaveAttribute('data-stage', 'demo_review');
    expect(
      bar.compareDocumentPosition(screen.getByTestId('composer-capsule')) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
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
