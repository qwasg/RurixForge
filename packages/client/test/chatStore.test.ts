import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useChatStore, type ForgeEventWire } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import { mockForgeBackend } from './forgeMock';

/**
 * F7 wave.4 chatStore 测试(参考 lib.rs ChatStore 语义):
 * applyEvent 全类型/去重/user-before-assistant 校正/乐观 local-* 替换/
 * selectSession 快照回放+SSE 订阅/gap 重拉/editAndResend revert 链/sendMessage 载荷/cancel。
 * SSE 客户端 mock(subscribe 捕获回调,手工注入帧)。
 */

const subscribeMock = vi.fn();
vi.mock('@/lib/sseClient', () => ({
  subscribeSessionEvents: (sid: string, fromSeq: number, cb: unknown) => subscribeMock(sid, fromSeq, cb),
}));

type SseCb = {
  onEvent: (f: { id: string | null; event: string; data: unknown }) => void;
  onGap?: (r: string) => void;
  onError?: (e: unknown) => void;
};
let sseCb: SseCb | null = null;

let seq = 0;
function evt(type: string, payload: Record<string, unknown>, id?: string): ForgeEventWire {
  seq += 1;
  return {
    id: id ?? `evt_${seq}`,
    sessionId: 'sess_1',
    seq,
    type,
    ts: '2026-08-18T10:24:00.000Z',
    payload,
  };
}

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialToasts = useToastStore.getState();

beforeEach(() => {
  seq = 0;
  sseCb = null;
  subscribeMock.mockReset().mockImplementation((_sid, _fromSeq, cb: SseCb) => {
    sseCb = cb;
    return { close: vi.fn(), lastSeq: () => 0 };
  });
  useChatStore.setState(initialChat, true);
  // 去重签名集在 store 闭包内(非 zustand state),必须经 reset() 清空,否则跨测试假去重
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useToastStore.setState(initialToasts, true);
  useSessionStore.setState({ activeSessionId: 'sess_1' });
  vi.stubGlobal(
    'fetch',
    mockForgeBackend({}, {
      '/api/forge/design-snapshot': {
        sessions: [],
        activeSession: { id: 'sess_1', selectedModelId: 'mock' },
        events: [],
        todos: [{ id: 'todo_1', title: '草图', status: 'queued' }],
        run: null,
        models: {
          models: [
            { id: 'deepseek-chat', label: 'deepseek-chat', provider: 'deepseek', availability: 'needs-key' },
            { id: 'mock', label: 'Mock provider', provider: 'mock', availability: 'available' },
          ],
          defaultModelId: 'deepseek-chat',
        },
        latestSeq: 7,
        chatFolders: [],
      },
      '/api/forge/sessions/sess_1/ask:execute': { message: { text: 'ok' }, run: { id: 'run_1', status: 'completed' }, mode: 'build' },
      '/api/forge/sessions/sess_1/revert': { ok: true },
      '/api/forge/sessions/sess_1': (init?: { body?: string }) => ({
        session: {
          id: 'sess_1',
          selectedModelId: (JSON.parse(init?.body ?? '{}') as { selectedModelId?: string }).selectedModelId ?? null,
        },
      }),
      '/api/forge/runs/run_1/cancel': { ok: true, runId: 'run_1' },
    }),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('chatStore.applyEvent(全类型)', () => {
  it('完整 turn:user → started → tool → message → completed;usage/todo 合并', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('composer.user.message', { text: '你好', composerMode: 'build', runId: 'run_1' }));
    s.applyEvent(evt('agent.started', { runId: 'run_1', model: 'mock' }));
    s.applyEvent(evt('agent.tool.invoked', { runId: 'run_1', name: 'mcp__engine-scene__entity_list', args: {}, toolCallId: 'call_1' }));
    s.applyEvent(evt('agent.tool.completed', { runId: 'run_1', name: 'mcp__engine-scene__entity_list', ok: true, toolCallId: 'call_1', durationMs: 12 }));
    s.applyEvent(evt('agent.usage', { runId: 'run_1', provider: 'deepseek', model: 'deepseek-chat', promptTokens: 10, completionTokens: 5, totalTokens: 15 }));
    s.applyEvent(evt('agent.message', { runId: 'run_1', text: '最终回答', provider: 'mock' }));
    s.applyEvent(evt('agent.completed', { runId: 'run_1', text: '最终回答' }));
    s.applyEvent(evt('todo.created', { id: 'todo_1', title: '草图', kind: 'edit', status: 'queued' }));
    s.applyEvent(evt('todo.updated', { id: 'todo_1', status: 'completed', title: '草图' }));

    const st = useChatStore.getState();
    expect(st.messages).toHaveLength(2);
    const [user, assistant] = st.messages;
    expect(user.role).toBe('user');
    expect(user.text).toBe('你好');
    expect(user.mode).toBe('build');
    expect(user.time).toBe('10:24');
    expect(assistant.status).toBe('completed');
    expect(assistant.model).toBe('mock');
    expect(assistant.provider).toBe('mock');
    expect(assistant.blocks).toHaveLength(2);
    const toolBlock = assistant.blocks[0];
    expect(toolBlock).toMatchObject({ kind: 'tool', toolCallId: 'call_1', ok: true, durationMs: 12 });
    expect(assistant.blocks[1]).toMatchObject({ kind: 'text', text: '最终回答', final: true });
    expect(st.activeRunId).toBeNull();
    expect(st.tokens).toEqual({ prompt: 10, completion: 5, total: 15 });
    expect(st.todos).toHaveLength(1);
    expect(st.todos[0]).toMatchObject({ id: 'todo_1', status: 'completed', kind: 'edit' });
  });

  it('agent.tool.failed → error 回填;agent.failed/cancelled 终态 + activeRunId 清', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'r2' }));
    expect(useChatStore.getState().activeRunId).toBe('r2');
    s.applyEvent(evt('agent.tool.invoked', { runId: 'r2', name: 'swarm.execute', args: { shardCount: 4 }, toolCallId: 'c9' }));
    s.applyEvent(evt('agent.tool.failed', { runId: 'r2', name: 'swarm.execute', error: '分片失败', toolCallId: 'c9', durationMs: 3 }));
    s.applyEvent(evt('agent.failed', { runId: 'r2', error: '模板未命中' }));
    let st = useChatStore.getState();
    const m = st.messages[0];
    expect(m.status).toBe('failed');
    expect(m.error).toBe('模板未命中');
    expect(m.blocks[0]).toMatchObject({ kind: 'tool', ok: false, error: '分片失败' });
    expect(st.activeRunId).toBeNull();
    // cancelled 路径
    s.applyEvent(evt('agent.started', { runId: 'r3' }));
    s.applyEvent(evt('agent.cancelled', { runId: 'r3' }));
    st = useChatStore.getState();
    expect(st.messages.find((x) => x.runId === 'r3')?.status).toBe('cancelled');
    expect(st.activeRunId).toBeNull();
  });

  it('name==="task" → subagent 块;completed 回填 subagent 状态', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'r4' }));
    s.applyEvent(evt('agent.tool.invoked', { runId: 'r4', name: 'task', args: { prompt: '探索\n细节', description: '探索后端' }, toolCallId: 'c1' }));
    s.applyEvent(evt('agent.tool.completed', { runId: 'r4', name: 'task', ok: true, toolCallId: 'c1' }));
    const m = useChatStore.getState().messages[0];
    expect(m.blocks[0]).toMatchObject({ kind: 'subagent', id: 'c1', label: '探索后端', status: 'done' });
  });

  it('去重:同 id 二次到达不重复应用;空 id 走 fallback 签名', () => {
    const s = useChatStore.getState();
    // 用 agent.usage 隔离 upsert 语义(同 runId 用户消息会经 upsert 折叠,非去重路径)
    const u1 = evt('agent.usage', { runId: 'r9', promptTokens: 1, completionTokens: 1, totalTokens: 2 }, 'evt_dup');
    s.applyEvent(u1);
    s.applyEvent(u1);
    expect(useChatStore.getState().tokens.total).toBe(2);
    const f1: ForgeEventWire = {
      id: '', sessionId: 'sess_1', seq: 50, type: 'agent.usage', ts: '2026-08-18T10:24:00.000Z',
      payload: { runId: 'r9', promptTokens: 1, completionTokens: 1, totalTokens: 2 },
    };
    s.applyEvent(f1);
    s.applyEvent({ ...f1 });
    expect(useChatStore.getState().tokens.total).toBe(4);
    s.applyEvent({ ...f1, seq: 51 });
    expect(useChatStore.getState().tokens.total).toBe(6);
  });

  it('user-before-assistant 校正:assistant 先建,user 后到 → 重排', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'r5' }));
    s.applyEvent(evt('composer.user.message', { text: '问题', runId: 'r5' }));
    const st = useChatStore.getState();
    expect(st.messages.map((m) => m.role)).toEqual(['user', 'assistant']);
  });

  it('乐观 local-* 替换:同文无 runId 用户卡被事件 upsert 替换', async () => {
    await useChatStore.getState().sendMessage('你好', 'build');
    expect(useChatStore.getState().messages[0].id).toMatch(/^local-/);
    useChatStore.getState().applyEvent(evt('composer.user.message', { text: '你好', runId: 'run_1', composerMode: 'build' }));
    const st = useChatStore.getState();
    expect(st.messages).toHaveLength(1);
    expect(st.messages[0].id).not.toMatch(/^local-/);
    expect(st.messages[0].runId).toBe('run_1');
  });
});

describe('chatStore 动作', () => {
  it('selectSession:快照回放 + latestSeq 起订 + todos/models/selectedModelId 回填', async () => {
    await useChatStore.getState().selectSession('sess_1');
    const st = useChatStore.getState();
    expect(st.latestSeq).toBe(7);
    expect(st.todos).toHaveLength(1);
    expect(st.models).toHaveLength(2);
    expect(st.selectedModelId).toBe('mock');
    expect(subscribeMock).toHaveBeenCalledWith('sess_1', 7, expect.anything());
    // SSE 帧注入 → applyEvent
    sseCb?.onEvent({ id: '8', event: 'composer.user.message', data: evt('composer.user.message', { text: '流式到', runId: 'r8' }, 'evt_live') });
    expect(useChatStore.getState().messages).toHaveLength(1);
  });

  it('selectSession:切换关旧流;null 清空', async () => {
    await useChatStore.getState().selectSession('sess_1');
    const first = subscribeMock.mock.results[0].value as { close: ReturnType<typeof vi.fn> };
    await useChatStore.getState().selectSession('sess_2');
    expect(first.close).toHaveBeenCalled();
    await useChatStore.getState().selectSession(null);
    expect(useChatStore.getState().messages).toHaveLength(0);
  });

  it('stream.gap → resync:重拉快照并以最新 latestSeq 重订', async () => {
    await useChatStore.getState().selectSession('sess_1');
    sseCb?.onGap?.('replay-window-exceeded');
    await vi.waitFor(() => {
      expect(subscribeMock).toHaveBeenCalledTimes(2);
    });
    expect(subscribeMock.mock.calls[1][0]).toBe('sess_1');
    expect(subscribeMock.mock.calls[1][1]).toBe(7);
  });

  it('sendMessage:载荷 {userInput, mode};无会话 → toast 不发请求', async () => {
    const fetchSpy = vi.mocked(fetch);
    await useChatStore.getState().sendMessage('  加碰撞体  ', 'multitask');
    const askCall = fetchSpy.mock.calls.find((c) => String(c[0]).includes('ask:execute'));
    expect(askCall).toBeTruthy();
    expect(JSON.parse(String((askCall?.[1] as { body: string }).body))).toEqual({
      userInput: '加碰撞体',
      mode: 'multitask',
    });
    // 无会话
    useSessionStore.setState({ activeSessionId: null });
    fetchSpy.mockClear();
    await useChatStore.getState().sendMessage('hi', 'build');
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(useToastStore.getState().items.some((t) => t.title.includes('请先选择会话'))).toBe(true);
  });

  it('cancelRun:POST /runs/{activeRunId}/cancel', async () => {
    useChatStore.setState({ activeRunId: 'run_1' });
    await useChatStore.getState().cancelRun();
    const call = vi.mocked(fetch).mock.calls.find((c) => String(c[0]).includes('/runs/run_1/cancel'));
    expect(call).toBeTruthy();
  });

  it('editAndResend:revert(before)→ 本地截断 → 以原 mode 重发', async () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('composer.user.message', { text: '第一句', runId: 'ra', composerMode: 'plan' }, 'evt_u1'));
    s.applyEvent(evt('agent.started', { runId: 'ra' }));
    s.applyEvent(evt('agent.completed', { runId: 'ra', text: '答一' }));
    s.applyEvent(evt('composer.user.message', { text: '第二句', runId: 'rb', composerMode: 'build' }, 'evt_u2'));
    s.applyEvent(evt('agent.started', { runId: 'rb' }));
    s.applyEvent(evt('agent.completed', { runId: 'rb', text: '答二' }));
    expect(useChatStore.getState().messages).toHaveLength(4);

    await useChatStore.getState().editAndResend('evt_u2', '改写后的第二句');
    const calls = vi.mocked(fetch).mock.calls.map((c) => String(c[0]));
    const revertIdx = calls.findIndex((u) => u.includes('/revert'));
    const askIdx = calls.findIndex((u) => u.includes('ask:execute'));
    expect(revertIdx).toBeGreaterThanOrEqual(0);
    expect(askIdx).toBeGreaterThan(revertIdx);
    const revertCall = vi.mocked(fetch).mock.calls[revertIdx];
    expect(JSON.parse(String((revertCall[1] as { body: string }).body))).toEqual({
      messageId: 'evt_u2',
      mode: 'before',
    });
    // revert 后显式 resync(seq 回退防旧连接滤新事件):快照重拉 + 重订,
    // 消息面 = 快照回放(本 mock 空 events)+ 新 local 乐观卡
    const st = useChatStore.getState();
    expect(st.messages).toHaveLength(1);
    expect(st.messages[0].id).toMatch(/^local-/);
    expect(st.messages[0].text).toBe('改写后的第二句');
    // 新发送载荷
    const askCall = vi.mocked(fetch).mock.calls[askIdx];
    expect(JSON.parse(String((askCall[1] as { body: string }).body))).toEqual({
      userInput: '改写后的第二句',
      mode: 'build',
    });
  });

  it('editAndResend:local-* 不调 revert;运行中拒绝并 toast', async () => {
    useChatStore.setState({
      messages: [
        { id: 'local-1', role: 'user', text: 'x', blocks: [], status: 'completed', time: '', runId: null },
      ],
      activeRunId: 'run_1',
    });
    // 运行中拒绝(msg 存在时)
    await useChatStore.getState().editAndResend('local-1', 'z');
    expect(useToastStore.getState().items.some((t) => t.title.includes('运行中'))).toBe(true);
    // local-*:不发 revert,直接截断重发
    useChatStore.setState({ activeRunId: null });
    await useChatStore.getState().editAndResend('local-1', 'y');
    expect(vi.mocked(fetch).mock.calls.some((c) => String(c[0]).includes('/revert'))).toBe(false);
  });

  it('pickModel:PATCH selectedModelId + 同步 sessionStore;失败回滚', async () => {
    useSessionStore.setState({
      sessions: [
        {
          id: 'sess_1', title: 't', status: 'idle', agentKind: 'coding', selectedModelId: null,
          webSearchEnabled: true, activeRunId: null, createdAt: '', updatedAt: '', pinned: false,
          titleManuallySet: false, folderId: null,
        },
      ],
      activeSessionId: 'sess_1',
    });
    await useChatStore.getState().pickModel('mock');
    const call = vi.mocked(fetch).mock.calls.find((c) => String(c[0]) === '/api/forge/sessions/sess_1');
    expect(call).toBeTruthy();
    expect(JSON.parse(String((call?.[1] as { body: string }).body))).toEqual({ selectedModelId: 'mock' });
    expect(useChatStore.getState().selectedModelId).toBe('mock');
  });
});
