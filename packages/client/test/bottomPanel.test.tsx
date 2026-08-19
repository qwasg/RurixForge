import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '@/App';
import BottomPanel, { summarizeEvent } from '@/components/shell/BottomPanel';
import { useChatStore, type ForgeEventWire } from '@/lib/chatStore';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { mockForgeBackend } from './forgeMock';

/**
 * F7 wave.5 底部面板:Agent Logs 120 截断 / Output 派生行 / Metrics 派生数 /
 * Ctrl+J 开关(Shell 接线)/ 拖拽 clamp / statusbar 钮。
 */

const initialChat = useChatStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
const initialSessions = useSessionStore.getState();
const initialOverlay = useOverlayStore.getState();

function evt(seq: number, type = 'agent.usage', payload: Record<string, unknown> = {}): ForgeEventWire {
  return {
    id: `evt_${seq}`,
    sessionId: 'sess_1',
    seq,
    type,
    ts: `2026-08-18T10:00:${String(seq % 60).padStart(2, '0')}.000Z`,
    payload,
  };
}

beforeEach(() => {
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useWorkbenchStore.setState(initialWorkbench, true);
  useSessionStore.setState(initialSessions, true);
  useOverlayStore.setState(initialOverlay, true);
  globalThis.localStorage?.clear();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('BottomPanel · Agent Logs', () => {
  it('事件环超 120 → 只渲染最近 120 条(截断如实)', () => {
    useChatStore.setState({ eventsRing: Array.from({ length: 130 }, (_, i) => evt(i + 1)) });
    useWorkbenchStore.setState({ bottomOpen: true, bottomTab: 'logs' });
    const { container } = render(<BottomPanel />);
    const rows = container.querySelectorAll('[data-testid^="log-row-"]');
    expect(rows.length).toBe(120);
    // 最新在前(倒序):首行 = seq 130
    expect(rows[0]).toHaveTextContent('#130');
    expect(rows[119]).toHaveTextContent('#11');
  });

  it('折叠树:展开 → payload pretty JSON(前 40 行)', () => {
    useChatStore.setState({
      eventsRing: [evt(1, 'composer.user.message', { text: '你好', composerMode: 'build', runId: 'r1' })],
    });
    useWorkbenchStore.setState({ bottomOpen: true, bottomTab: 'logs' });
    render(<BottomPanel />);
    expect(screen.queryByTestId('log-payload-1')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('log-row-1'));
    const payload = screen.getByTestId('log-payload-1');
    expect(payload).toHaveTextContent('"text": "你好"');
    expect(payload).toHaveTextContent('"composerMode": "build"');
  });

  it('空环:(暂无事件)', () => {
    useWorkbenchStore.setState({ bottomOpen: true, bottomTab: 'logs' });
    render(<BottomPanel />);
    expect(screen.getByText('（暂无事件）')).toBeInTheDocument();
  });
});

describe('BottomPanel · Output', () => {
  it('派生行:时间 + type + 一句话摘要', () => {
    useChatStore.setState({
      eventsRing: [
        evt(1, 'composer.user.message', { text: '给我搭个场景', composerMode: 'build' }),
        evt(2, 'agent.tool.invoked', { name: 'mcp__engine-scene__entity_create', args: {} }),
        evt(3, 'agent.completed', { text: '完成' }),
      ],
    });
    useWorkbenchStore.setState({ bottomOpen: true, bottomTab: 'output' });
    render(<BottomPanel />);
    const lines = screen.getByTestId('output-lines');
    expect(lines).toHaveTextContent('composer.user.message');
    expect(lines).toHaveTextContent('用户消息(build):给我搭个场景');
    expect(lines).toHaveTextContent('工具调用:mcp__engine-scene__entity_create');
    expect(lines).toHaveTextContent('run 完成');
    expect(lines).toHaveTextContent('10:00:');
  });

  it('summarizeEvent 全类型覆盖(未知类型回退 type 名)', () => {
    expect(summarizeEvent(evt(1, 'agent.failed', { error: '炸了' }))).toBe('run 失败:炸了');
    expect(summarizeEvent(evt(2, 'todo.updated', { id: 'todo_1', status: 'completed' }))).toBe(
      '待办更新:todo_1 → completed',
    );
    expect(summarizeEvent(evt(3, 'some.unknown.type'))).toBe('some.unknown.type');
  });
});

describe('BottomPanel · Metrics', () => {
  it('本地派生卡:Total tokens / Tool calls / Sessions / Run 状态', () => {
    useChatStore.setState({
      tokens: { prompt: 100, completion: 21, total: 121 },
      messages: [
        {
          id: 'a1',
          role: 'assistant',
          text: '',
          time: '',
          blocks: [
            { kind: 'tool', toolCallId: 't1', name: 'x', args: '', mcp: null },
            { kind: 'tool', toolCallId: 't2', name: 'y', args: '', mcp: null },
            { kind: 'text', text: 'done', final: true },
          ],
        },
      ],
      todos: [
        { id: 't1', title: 'a', status: 'completed' },
        { id: 't2', title: 'b', status: 'queued' },
      ],
      activeRunId: null,
    });
    useWorkbenchStore.setState({ bottomOpen: true, bottomTab: 'metrics' });
    render(<BottomPanel />);
    expect(screen.getByTestId('metric-tokens')).toHaveTextContent('121');
    expect(screen.getByTestId('metric-toolcalls')).toHaveTextContent('2');
    expect(screen.getByTestId('metric-sessions')).toHaveTextContent('1/2');
    expect(screen.getByTestId('metric-run')).toHaveTextContent('idle');
  });
});

describe('BottomPanel · 开关与拖拽', () => {
  it('Ctrl+J 开关(App 全局快捷键) + statusbar 钮', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        { entity_list: { entities: [] } },
        {
          '/api/forge/sessions': { sessions: [] },
          '/api/forge/chat-folders': { folders: [] },
          '/api/forge/health': { status: 'ok' },
          '/api/forge/design-snapshot': { sessions: [], activeSession: null, events: [], todos: [], run: null, models: { models: [] }, latestSeq: 0, chatFolders: [] },
          '/api/forge/workspace/tree': { path: '', entries: [], total: 0, truncated: false },
        },
      ),
    );
    render(<App />);
    expect(screen.queryByTestId('bottom-panel')).not.toBeInTheDocument();
    // Ctrl+J 开
    fireEvent.keyDown(window, { key: 'j', ctrlKey: true });
    expect(useWorkbenchStore.getState().bottomOpen).toBe(true);
    expect(await screen.findByTestId('bottom-panel')).toBeInTheDocument();
    // Ctrl+J 关
    fireEvent.keyDown(window, { key: 'j', ctrlKey: true });
    expect(screen.queryByTestId('bottom-panel')).not.toBeInTheDocument();
    // statusbar 钮开
    fireEvent.click(screen.getByTestId('statusbar-bottom-toggle'));
    expect(await screen.findByTestId('bottom-panel')).toBeInTheDocument();
    expect(useWorkbenchStore.getState().bottomOpen).toBe(true);
  });

  it('顶部拖拽条:mousemove 实时改高(clamp 120–520)', () => {
    useWorkbenchStore.setState({ bottomOpen: true, bottomH: 260 });
    render(<BottomPanel />);
    const drag = screen.getByTestId('bottom-drag');
    fireEvent.mouseDown(drag, { clientY: 500 });
    fireEvent.mouseMove(window, { clientY: 400 }); // 上移 100 → 高 360
    expect(useWorkbenchStore.getState().bottomH).toBe(360);
    fireEvent.mouseMove(window, { clientY: -2000 }); // 超大 → clamp 520
    expect(useWorkbenchStore.getState().bottomH).toBe(520);
    fireEvent.mouseMove(window, { clientY: 3000 }); // 超小 → clamp 120
    expect(useWorkbenchStore.getState().bottomH).toBe(120);
    fireEvent.mouseUp(window);
  });

  it('关闭钮 = toggleBottom', () => {
    useWorkbenchStore.setState({ bottomOpen: true });
    render(<BottomPanel />);
    fireEvent.click(screen.getByTestId('bottom-close'));
    expect(useWorkbenchStore.getState().bottomOpen).toBe(false);
  });
});

describe('chatStore eventsRing', () => {
  it('applyEvent 全类型入环;clearEventsRing 清空;cap 200 FIFO', () => {
    const st = useChatStore.getState();
    act(() => {
      st.applyEvent(evt(1, 'session.created'));
      st.applyEvent(evt(2, 'agent.usage', { totalTokens: 5 }));
    });
    expect(useChatStore.getState().eventsRing.length).toBe(2);
    // cap:灌 210 → 留 200,首条 = seq 11
    act(() => {
      for (let i = 3; i <= 212; i += 1) useChatStore.getState().applyEvent(evt(i));
    });
    const ring = useChatStore.getState().eventsRing;
    expect(ring.length).toBe(200);
    expect(ring[0].seq).toBe(13);
    act(() => useChatStore.getState().clearEventsRing());
    expect(useChatStore.getState().eventsRing.length).toBe(0);
  });
});
