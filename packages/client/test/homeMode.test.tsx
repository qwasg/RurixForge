import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ChatColumn from '@/components/shell/ChatColumn';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import { HOME_INPUT_MIN } from '@/lib/chatVariant';
import { useComposerPrefillStore } from '@/lib/composerStore';
import { useSessionStore, type ForgeSession } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { mockForgeBackend } from './forgeMock';

/**
 * 全屏对话主页(2026-08-24 用户拍板「workbench 空态 → 像 Codex 一样全屏输入」):
 * useHomeMode 判定 / hero 首屏与起手式 / 大输入盒 / 无会话直发(建会话再发) /
 * 有消息后 hero 让位消息流。三栏 ↔ 主页的壳级切换在 shell.test.tsx。
 */

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
const initialPrefill = useComposerPrefillStore.getState();

function session(id: string): ForgeSession {
  return {
    id,
    title: '',
    status: 'idle',
    agentKind: 'coding',
    selectedModelId: null,
    thinkingEnabled: false,
    reasoningEffort: null,
    contextOptionId: null,
    webSearchEnabled: true,
    activeRunId: null,
    createdAt: '',
    updatedAt: '',
    pinned: false,
    titleManuallySet: false,
    folderId: null,
  };
}

function msg(partial: Partial<ChatMsg> & Pick<ChatMsg, 'id' | 'role'>): ChatMsg {
  return { text: '', blocks: [], time: '', ...partial };
}

beforeEach(() => {
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  useComposerPrefillStore.setState(initialPrefill, true);
  globalThis.localStorage?.clear();
  vi.stubGlobal('fetch', mockForgeBackend({}, { '/api/forge/skills/list': { skills: [] } }));
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('useHomeMode 判定', () => {
  const home = () => useWorkbenchStore.getState().tabs.length === 0 && !useWorkbenchStore.getState().homeDismissed;

  it('无 tab 即主页;开 tab 退出;关光 tab 自动回来', () => {
    expect(home()).toBe(true);
    useWorkbenchStore.getState().openEditor();
    expect(home()).toBe(false);
    useWorkbenchStore.getState().closeTab('editor');
    expect(home()).toBe(true);
  });

  it('手动「工作台」暂避;再开任一 tab 复位暂避态', () => {
    useWorkbenchStore.getState().setHomeDismissed(true);
    expect(home()).toBe(false);
    useWorkbenchStore.getState().openTab('plan');
    expect(useWorkbenchStore.getState().homeDismissed).toBe(false);
    useWorkbenchStore.getState().closeTab('plan');
    expect(home()).toBe(true);
  });

  it('openFile 同样复位暂避态', () => {
    useWorkbenchStore.getState().setHomeDismissed(true);
    useWorkbenchStore.getState().openFile('src/main.rs');
    expect(useWorkbenchStore.getState().homeDismissed).toBe(false);
  });
});

describe('<ChatColumn variant="home" /> 首屏', () => {
  it('零消息:hero + 起手式 + 大输入盒(底高 HOME_INPUT_MIN,非胶囊)', () => {
    render(<ChatColumn variant="home" />);
    expect(screen.getByTestId('home-hero')).toHaveTextContent('今天想搭点什么？');
    expect(screen.getByTestId('home-quick-starts')).toBeInTheDocument();
    expect(screen.getByTestId('home-to-workbench')).toBeInTheDocument();
    // 消息流让位给 hero
    expect(screen.queryByTestId('message-list')).not.toBeInTheDocument();
    // 输入壳以多行盒起步:composer 不是胶囊态,壳高 = HOME_INPUT_MIN
    const composer = screen.getByTestId('composer');
    expect(composer).toHaveAttribute('data-variant', 'home');
    expect(composer).not.toHaveAttribute('data-capsule');
    expect(screen.getByTestId('composer-capsule').className).toContain('rounded-2xl');
    const shell = screen.getByTestId('composer-input').parentElement as HTMLElement;
    expect(shell.style.height).toBe(`${HOME_INPUT_MIN}px`);
  });

  it('起手式 chip → 预填输入框并切模式(不直接发)', () => {
    render(<ChatColumn variant="home" />);
    fireEvent.click(screen.getByTestId('home-quick-plan'));
    expect(screen.getByTestId('composer-input')).toHaveValue('给我一份从零做出可玩 demo 的分步计划');
    expect(screen.getByTestId('composer-mode-chip')).toHaveTextContent('Plan');
  });

  it('有消息:hero 收起,消息流铺开', () => {
    useSessionStore.setState({ activeSessionId: 'sess_1', sessions: [session('sess_1')] });
    useChatStore.setState({
      currentSessionId: 'sess_1',
      messages: [msg({ id: 'u1', role: 'user', text: '搭个平台' })],
    });
    render(<ChatColumn variant="home" />);
    expect(screen.queryByTestId('home-hero')).not.toBeInTheDocument();
    expect(screen.getByTestId('message-list')).toHaveTextContent('搭个平台');
  });
});

describe('<ChatColumn variant="home" /> 无会话直发', () => {
  it('没有会话也能发:先建会话 → 订阅 → 再发,warn 胶囊换成「发送即新建会话」', async () => {
    const created = session('sess_new');
    const create = vi.fn(async () => {
      useSessionStore.setState({ activeSessionId: created.id, sessions: [created] });
      return created;
    });
    const selectSession = vi.fn(async () => {});
    const sendMessage = vi.fn(async () => {});
    useSessionStore.setState({ create });
    useChatStore.setState({ selectSession, sendMessage });

    render(<ChatColumn variant="home" />);
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '做个迷宫' } });
    expect(screen.getByTestId('composer-hint-new-session')).toBeInTheDocument();
    expect(screen.queryByTestId('composer-warn-no-session')).not.toBeInTheDocument();
    const sendBtn = screen.getByTestId('composer-send');
    expect(sendBtn).toBeEnabled();
    fireEvent.click(sendBtn);

    await vi.waitFor(() => expect(sendMessage).toHaveBeenCalledWith('做个迷宫', 'build'));
    expect(create).toHaveBeenCalled();
    expect(selectSession).toHaveBeenCalledWith('sess_new');
  });

  it('建会话失败(后端不可达)不发消息', async () => {
    const create = vi.fn(async () => null);
    const sendMessage = vi.fn(async () => {});
    useSessionStore.setState({ create });
    useChatStore.setState({ sendMessage });

    render(<ChatColumn variant="home" />);
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '做个迷宫' } });
    fireEvent.click(screen.getByTestId('composer-send'));
    await vi.waitFor(() => expect(create).toHaveBeenCalled());
    expect(sendMessage).not.toHaveBeenCalled();
  });
});

describe('列变体不受影响', () => {
  it('column:仍是胶囊 + 无会话禁发 + 折叠钮', () => {
    render(<ChatColumn />);
    const composer = screen.getByTestId('composer');
    expect(composer).toHaveAttribute('data-variant', 'column');
    expect(composer).toHaveAttribute('data-capsule', '1');
    expect(screen.getByTestId('pane-toggle-chat')).toBeInTheDocument();
    expect(screen.queryByTestId('home-to-workbench')).not.toBeInTheDocument();
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '做个迷宫' } });
    expect(screen.getByTestId('composer-send')).toBeDisabled();
    expect(screen.getByTestId('composer-warn-no-session')).toBeInTheDocument();
  });
});
