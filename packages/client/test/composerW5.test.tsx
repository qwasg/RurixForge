import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Composer from '@/components/chat/Composer';
import { useChatStore } from '@/lib/chatStore';
import { useComposerPrefillStore } from '@/lib/composerStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { mockForgeBackend } from './forgeMock';

/** F7 wave.5 Composer 收尾:TodoStrip 就地展开精简面板 / Ctrl+Enter 发送设置消费 / 外部预填 seam。 */

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialSettings = useSettingsStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
const initialPrefill = useComposerPrefillStore.getState();

beforeEach(() => {
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useSettingsStore.setState(initialSettings, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  useComposerPrefillStore.setState(initialPrefill, true);
  useSessionStore.setState({ activeSessionId: 'sess_1' });
  globalThis.localStorage?.clear();
  vi.stubGlobal('fetch', mockForgeBackend({}, {}));
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('Composer wave.5 接线', () => {
  it('TodoStrip「展开」→ 就地向上展开精简列表(不再开 workbench tab)', () => {
    useChatStore.setState({ todos: [{ id: 't1', title: '改场景', status: 'queued' }] });
    render(<Composer />);
    // 默认折叠无行
    expect(screen.queryByTestId('todo-row-t1')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('todo-strip-toggle'));
    // 就地展开,精简行出现
    expect(screen.getByTestId('todo-row-t1')).toHaveTextContent('改场景');
    // 不再跳转 workbench todo tab
    expect(useWorkbenchStore.getState().tabs.some((t) => t.id === 'todo')).toBe(false);
    // 再点收起
    fireEvent.click(screen.getByTestId('todo-strip-toggle'));
    expect(screen.queryByTestId('todo-row-t1')).not.toBeInTheDocument();
  });

  it('Ctrl+Enter 发送开启:Enter 换行(不发送),Ctrl+Enter 发送', () => {
    useSettingsStore.getState().setSubmitCtrlEnter(true);
    const sendMessage = vi.fn();
    useChatStore.setState({ sendMessage });
    render(<Composer />);
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '你好' } });
    // Enter 不发送
    fireEvent.keyDown(screen.getByTestId('composer-input'), { key: 'Enter' });
    expect(sendMessage).not.toHaveBeenCalled();
    // Ctrl+Enter 发送
    fireEvent.keyDown(screen.getByTestId('composer-input'), { key: 'Enter', ctrlKey: true });
    expect(sendMessage).toHaveBeenCalledWith('你好', 'build');
  });

  it('Ctrl+Enter 关闭(默认):Enter 发送(回归)', () => {
    const sendMessage = vi.fn();
    useChatStore.setState({ sendMessage });
    render(<Composer />);
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '你好' } });
    fireEvent.keyDown(screen.getByTestId('composer-input'), { key: 'Enter' });
    expect(sendMessage).toHaveBeenCalledWith('你好', 'build');
  });

  it('外部预填 seam:prefill → 文本灌入 + 模式切换 + store 一次性消费', () => {
    render(<Composer />);
    act(() => {
      useComposerPrefillStore.getState().prefill('执行上述计划', 'plan');
    });
    expect(screen.getByTestId('composer-input')).toHaveValue('执行上述计划');
    // plan 非 build → 模式 chip 出现
    expect(screen.getByTestId('composer-mode-chip')).toHaveTextContent('Plan');
    // 一次性消费
    expect(useComposerPrefillStore.getState().draft).toBeNull();
  });
});
