import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  SESSION_SNAPSHOT_RETRY_MS,
  useChatStore,
  type ForgeEventWire,
} from '@/lib/chatStore';
import { useGoalStore } from '@/lib/goalStore';
import { usePlanStore } from '@/lib/planStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import { useUltraPlanStore } from '@/lib/ultraPlanStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
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
  onOpen?: () => void;
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
const initialUltra = useUltraPlanStore.getState();

beforeEach(() => {
  seq = 0;
  sseCb = null;
  subscribeMock.mockReset().mockImplementation((_sid, _fromSeq, cb: SseCb) => {
    sseCb = cb;
    return { close: vi.fn(), lastSeq: () => 0 };
  });
  // D-044:UltraPlan 用例会把 store 动作换成 spy,整体还原后再由下面的 chat reset() 清闭包态
  useUltraPlanStore.setState(initialUltra, true);
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
      '/api/forge/sessions/sess_1/goal': { goal: null, engine: 'local' },
      '/api/forge/permissions/': { ok: true },
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

describe('Codex 原生图片事件', () => {
  it('回放保存的图片引用，重复图片不产生重复卡片', () => {
    const store = useChatStore.getState();
    store.applyEvent(evt('agent.started', { runId: 'image-run', engine: 'codex' }));
    const payload = { runId: 'image-run', toolCallId: 'image-call', url: '/api/forge/gen/image/file?fileRef=.forge/tmp/gen/image.png&workspaceId=ws_a', imageFileRef: '.forge/tmp/gen/image.png', revisedPrompt: 'a red circle' };
    store.applyEvent(evt('agent.image.generated', payload));
    store.applyEvent(evt('agent.image.generated', payload));
    store.applyEvent(evt('agent.completed', { runId: 'image-run' }));
    const message = useChatStore.getState().messages.find((m) => m.runId === 'image-run');
    expect(message?.blocks.filter((b) => b.kind === 'image')).toEqual([
      { kind: 'image', toolCallId: 'image-call', url: payload.url, imageFileRef: payload.imageFileRef, revisedPrompt: 'a red circle' },
    ]);
    expect(message?.status).toBe('completed');
  });

  it('缺少文件引用或包含非图片端点的事件不渲染', () => {
    const store = useChatStore.getState();
    store.applyEvent(evt('agent.started', { runId: 'image-run' }));
    store.applyEvent(evt('agent.image.generated', { runId: 'image-run', toolCallId: 'bad', url: 'javascript:alert(1)', imageFileRef: 'x.png' }));
    store.applyEvent(evt('agent.image.generated', { runId: 'image-run', toolCallId: 'missing', url: '/api/forge/gen/image/file?fileRef=x.png' }));
    expect(useChatStore.getState().messages.flatMap((m) => m.blocks).some((b) => b.kind === 'image')).toBe(false);
  });
});

describe('chatStore.applyEvent(全类型)', () => {
  it('丢弃已切走会话队列中残留的 SSE 帧', () => {
    useSessionStore.setState({ activeSessionId: 'sess_2' });
    useChatStore.setState({ currentSessionId: 'sess_2' });
    useChatStore.getState().applyEvent(evt('agent.started', { runId: 'stale-run' }));
    expect(useChatStore.getState().messages).toEqual([]);
    expect(useChatStore.getState().eventsRing).toEqual([]);
  });

  it('完整 turn:user → started → tool → message → completed;usage/todo 合并', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('composer.user.message', { text: '你好', composerMode: 'build', runId: 'run_1' }));
    s.applyEvent(evt('agent.started', { runId: 'run_1', model: 'mock', engine: 'codex' }));
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
    expect(assistant.engine).toBe('codex');
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

  it('流式 delta + message 终稿;stream.reset 清半截字;args.delta 拼 args', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'rs' }));
    s.applyEvent(evt('agent.reasoning.delta', { runId: 'rs', delta: '想' }));
    s.applyEvent(evt('agent.token.stream.delta', { runId: 'rs', delta: '你' }));
    s.applyEvent(evt('agent.token.stream.delta', { runId: 'rs', delta: '好' }));
    let m = useChatStore.getState().messages[0];
    expect(m.blocks.find((b) => b.kind === 'reasoning')).toMatchObject({ kind: 'reasoning', text: '想' });
    expect(m.blocks.find((b) => b.kind === 'text')).toMatchObject({ kind: 'text', text: '你好', final: false });
    s.applyEvent(evt('agent.message', { runId: 'rs', text: '你好世界', provider: 'mock' }));
    m = useChatStore.getState().messages[0];
    expect(m.blocks.find((b) => b.kind === 'text')).toMatchObject({ kind: 'text', text: '你好世界', final: true });
    s.applyEvent(evt('agent.started', { runId: 'rr' }));
    s.applyEvent(evt('agent.token.stream.delta', { runId: 'rr', delta: '半' }));
    s.applyEvent(evt('agent.stream.reset', { runId: 'rr' }));
    const rr = useChatStore.getState().messages.find((x) => x.runId === 'rr');
    expect(rr?.blocks.some((b) => b.kind === 'text' && b.kind === 'text' && !b.final && b.text === '半')).toBe(false);
    s.applyEvent(evt('agent.tool.args.delta', { runId: 'rr', toolCallId: 'cΔ', name: 'read_file', delta: '{"p' }));
    s.applyEvent(evt('agent.tool.args.delta', { runId: 'rr', toolCallId: 'cΔ', name: 'read_file', delta: 'ath"}' }));
    const tool = useChatStore.getState().messages.find((x) => x.runId === 'rr')?.blocks.find((b) => b.kind === 'tool');
    expect(tool).toMatchObject({ kind: 'tool', args: '{"path"}' });
  });

  it('思考块计时:首末 reasoning 事件 ts 入块,终稿只推进末 ts', () => {
    const at = (ts: string, type: string, payload: Record<string, unknown>) => ({ ...evt(type, payload), ts });
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'rt' }));
    s.applyEvent(at('2026-08-18T10:24:00.000Z', 'agent.reasoning.delta', { runId: 'rt', delta: '想' }));
    s.applyEvent(at('2026-08-18T10:24:09.000Z', 'agent.reasoning.delta', { runId: 'rt', delta: '好' }));
    const think = () =>
      useChatStore.getState().messages.find((x) => x.runId === 'rt')?.blocks.find((b) => b.kind === 'reasoning');
    expect(think()).toMatchObject({
      text: '想好',
      startedTs: '2026-08-18T10:24:00.000Z',
      endedTs: '2026-08-18T10:24:09.000Z',
    });
    s.applyEvent(at('2026-08-18T10:24:11.000Z', 'agent.reasoning', { runId: 'rt', text: '想好了' }));
    expect(think()).toMatchObject({
      text: '想好了',
      startedTs: '2026-08-18T10:24:00.000Z',
      endedTs: '2026-08-18T10:24:11.000Z',
    });
  });

  it('思考流完再吐正文后收到 reasoning 终稿:不另开第二套 Thought/正文', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'rdup' }));
    s.applyEvent(evt('agent.reasoning.delta', { runId: 'rdup', delta: '想一下' }));
    s.applyEvent(evt('agent.token.stream.delta', { runId: 'rdup', delta: '你好' }));
    s.applyEvent(evt('agent.reasoning', { runId: 'rdup', text: '想一下' }));
    s.applyEvent(evt('agent.message', { runId: 'rdup', text: '你好，需要我帮你制作 2D 场景吗？' }));
    s.applyEvent(evt('agent.completed', { runId: 'rdup', text: '你好，需要我帮你制作 2D 场景吗？' }));
    const blocks = useChatStore.getState().messages.find((m) => m.runId === 'rdup')?.blocks ?? [];
    expect(blocks.filter((b) => b.kind === 'reasoning')).toHaveLength(1);
    expect(blocks.filter((b) => b.kind === 'text')).toHaveLength(1);
    expect(blocks.find((b) => b.kind === 'text')).toMatchObject({
      text: '你好，需要我帮你制作 2D 场景吗？',
      final: true,
    });
  });

  it('completed 带 output → result;denied 标失败;permission.requested 同时进审批块与 toast', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'ro' }));
    s.applyEvent(evt('agent.tool.invoked', { runId: 'ro', name: 'read_file', args: { path: 'a.txt' }, toolCallId: 'c1' }));
    s.applyEvent(evt('agent.tool.completed', { runId: 'ro', toolCallId: 'c1', ok: true, output: 'hello\nworld', durationMs: 4 }));
    s.applyEvent(evt('agent.tool.invoked', { runId: 'ro', name: 'write_file', args: { path: 'b.txt' }, toolCallId: 'c2' }));
    s.applyEvent(evt('agent.tool.denied', { runId: 'ro', toolCallId: 'c2', error: 'TOOL_FORBIDDEN: write_file' }));
    s.applyEvent(evt('permission.requested', {
      id: 'perm_1',
      tool: 'shell',
      kind: 'command',
      runId: 'ro',
      command: 'pnpm test',
      cwd: 'D:/repo',
      reason: '需要执行测试',
      availableDecisions: ['accept', 'acceptForSession', 'decline'],
    }));
    const st = useChatStore.getState();
    const tools = st.messages[0].blocks.filter((b) => b.kind === 'tool');
    expect(tools[0]).toMatchObject({ result: 'hello\nworld', ok: true });
    expect(tools[1]).toMatchObject({ ok: false, error: 'TOOL_FORBIDDEN: write_file' });
    expect(st.pendingPermission).toMatchObject({
      id: 'perm_1',
      tool: 'shell',
      approvalKind: 'command',
      command: 'pnpm test',
    });
    expect(st.messages[0].blocks.find((block) => block.kind === 'approval')).toMatchObject({
      kind: 'approval',
      id: 'perm_1',
      approvalKind: 'command',
      command: 'pnpm test',
    });
    expect(useToastStore.getState().items.some((t) => t.title.includes('需要批准'))).toBe(true);
  });

  it('轮终止时收束缺失 resolved 的历史审批，不留可点幽灵卡', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'stale-approval-run' }));
    s.applyEvent(evt('permission.requested', {
      id: 'perm-stale', runId: 'stale-approval-run', kind: 'command', tool: 'shell',
    }));
    s.applyEvent(evt('agent.failed', {
      runId: 'stale-approval-run', error: 'agentd 进程重启',
    }));

    expect(useChatStore.getState().pendingPermission).toBeNull();
    expect(
      useChatStore.getState().messages[0].blocks.find((block) => block.kind === 'approval'),
    ).toMatchObject({ id: 'perm-stale', decision: 'expired' });
  });

  it('Codex 工具输出/文件变更/plan/diff 事件映射到时间线块', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'codex-run', engine: 'codex' }));
    s.applyEvent(evt('agent.tool.invoked', {
      runId: 'codex-run',
      name: 'shell',
      toolKind: 'command',
      toolCallId: 'cmd-1',
      args: { command: 'pnpm test', cwd: 'D:/repo' },
    }));
    s.applyEvent(evt('agent.tool.output.delta', {
      runId: 'codex-run', toolCallId: 'cmd-1', delta: 'first\n',
    }));
    s.applyEvent(evt('agent.tool.output.delta', {
      runId: 'codex-run', toolCallId: 'cmd-1', delta: 'second\n',
    }));
    s.applyEvent(evt('agent.tool.completed', {
      runId: 'codex-run', name: 'shell', toolKind: 'command', toolCallId: 'cmd-1',
      output: 'first\nsecond\n', exitCode: 0,
    }));
    s.applyEvent(evt('agent.tool.invoked', {
      runId: 'codex-run', name: 'apply_patch', toolKind: 'fileChange', toolCallId: 'edit-1',
      args: { changes: [{ path: 'src/a.ts', kind: 'update' }] },
    }));
    s.applyEvent(evt('agent.diff.updated', { runId: 'codex-run', diff: '@@\n-old\n+new' }));
    s.applyEvent(evt('agent.plan.delta', { runId: 'codex-run', delta: '先审计' }));
    s.applyEvent(evt('agent.plan.delta', { runId: 'codex-run', delta: '，再修改' }));
    s.applyEvent(evt('plan.created', { runId: 'codex-run', path: '.forge/plans/codex.plan.md' }));

    const message = useChatStore.getState().messages[0];
    expect(message.blocks.find((block) => block.kind === 'tool' && block.toolCallId === 'cmd-1')).toMatchObject({
      toolKind: 'command', output: 'first\nsecond\n', exitCode: 0, status: 'done',
    });
    expect(message.blocks.find((block) => block.kind === 'tool' && block.toolCallId === 'edit-1')).toMatchObject({
      toolKind: 'fileChange',
      changes: [{ path: 'src/a.ts', kind: 'update', diff: '@@\n-old\n+new' }],
    });
    expect(message.blocks.find((block) => block.kind === 'plan')).toMatchObject({
      kind: 'plan', text: '先审计，再修改', final: true, planPath: '.forge/plans/codex.plan.md',
    });
    expect(useChatStore.getState().latestDiff).toBe('@@\n-old\n+new');
  });

  it('审批 resolve 携 decision/answers，resolved 回填内联卡片', async () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'approval-run' }));
    s.applyEvent(evt('permission.requested', {
      id: 'perm-input', runId: 'approval-run', kind: 'userInput', tool: 'request_user_input',
      questions: [{
        id: 'scope', header: '范围', question: '选择范围', required: true,
        secret: false, isSecret: true, isOther: true,
        options: [{ label: '全部' }, { label: '其他', description: '自行填写', isOther: true }],
      }],
      permissions: { network: { enabled: true } },
      schema: {
        type: 'object',
        properties: {
          enabled: { type: 'boolean' },
          retries: { type: 'integer' },
        },
        required: ['enabled'],
      },
      networkApprovalContext: { host: 'api.example.com', protocol: 'https' },
      proposedExecpolicyAmendment: ['cargo', 'test'],
      proposedNetworkPolicyAmendments: [{ host: 'api.example.com', action: 'allow' }],
      grantRoot: 'D:/RurixForge',
      mode: 'url',
      serverName: 'deploy-mcp',
      url: 'https://auth.example.com/continue',
      elicitationId: 'elicit-1',
    }));
    expect(useChatStore.getState().pendingPermission).toMatchObject({
      questions: [{
        id: 'scope', required: true, secret: false, isSecret: true, isOther: true,
        options: [{ label: '全部' }, { label: '其他', isOther: true }],
      }],
      permissions: { network: { enabled: true } },
      schema: { type: 'object', required: ['enabled'] },
      networkApprovalContext: { host: 'api.example.com', protocol: 'https' },
      proposedExecpolicyAmendment: ['cargo', 'test'],
      proposedNetworkPolicyAmendments: [{ host: 'api.example.com', action: 'allow' }],
      grantRoot: 'D:/RurixForge',
      mode: 'url',
      serverName: 'deploy-mcp',
      url: 'https://auth.example.com/continue',
      elicitationId: 'elicit-1',
    });
    expect(useChatStore.getState().messages[0].blocks.find((block) => block.kind === 'approval')).toMatchObject({
      permissions: { network: { enabled: true } },
      schema: { type: 'object', required: ['enabled'] },
      grantRoot: 'D:/RurixForge',
      serverName: 'deploy-mcp',
    });
    await useChatStore.getState().resolvePermission('perm-input', true, {
      decision: 'acceptForSession',
      answers: { scope: ['全部'], enabled: true, retries: 2 },
    });
    const call = vi.mocked(fetch).mock.calls.find((entry) =>
      String(entry[0]).includes('/permissions/perm-input/approve'),
    );
    expect(JSON.parse(String((call?.[1] as { body: string }).body))).toEqual({
      decision: 'acceptForSession',
      answers: { scope: ['全部'], enabled: true, retries: 2 },
    });
    s.applyEvent(evt('permission.resolved', {
      id: 'perm-input', runId: 'approval-run', allowed: true, decision: 'acceptForSession',
      answers: { scope: ['全部'], enabled: true, retries: 2 },
    }));
    expect(useChatStore.getState().pendingPermission).toBeNull();
    expect(useChatStore.getState().messages[0].blocks.find((block) => block.kind === 'approval')).toMatchObject({
      decision: 'acceptForSession',
      answers: { scope: ['全部'], enabled: true, retries: 2 },
    });
  });

  it('goal/codex account 事件进入专用 store，并且首个实时 goal 自动开页签', () => {
    useWorkbenchStore.setState({ tabs: [], activeTabId: null });
    const s = useChatStore.getState();
    s.applyEvent(evt('goal.updated', {
      engine: 'codex',
      goal: {
        objective: '交付 Codex 接入', status: 'active', tokenBudget: 1000,
        tokensUsed: 125, timeUsedSeconds: 9, turns: 1,
      },
    }));
    s.applyEvent(evt('codex.rateLimits.updated', { rateLimits: { primary: { usedPercent: 25 } } }));
    s.applyEvent(evt('codex.account.updated', { account: { authMode: 'chatgpt', planType: 'pro' } }));
    expect(useGoalStore.getState().goal).toMatchObject({
      objective: '交付 Codex 接入', engine: 'codex', tokensUsed: 125,
    });
    expect(useWorkbenchStore.getState().tabs.some((tab) => tab.kind === 'goal')).toBe(true);
    expect(useChatStore.getState().codexRateLimits).toMatchObject({ primary: { usedPercent: 25 } });
    expect(useChatStore.getState().codexAccount).toMatchObject({ authMode: 'chatgpt', planType: 'pro' });
  });

  it('name==="task" → subagent 块;completed 回填 subagent 状态', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'r4' }));
    s.applyEvent(evt('agent.tool.invoked', { runId: 'r4', name: 'task', args: { prompt: '探索\n细节', description: '探索后端' }, toolCallId: 'c1' }));
    s.applyEvent(evt('agent.tool.completed', { runId: 'r4', name: 'task', ok: true, toolCallId: 'c1' }));
    const m = useChatStore.getState().messages[0];
    expect(m.blocks[0]).toMatchObject({ kind: 'subagent', id: 'c1', label: '探索后端', status: 'done' });
  });

  it('子代理 work 嵌套 parentToolCallId + subagent.started', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'rp' }));
    s.applyEvent(evt('agent.tool.invoked', { runId: 'rp', name: 'task', args: { prompt: '查实体', description: '探索' }, toolCallId: 'sub1' }));
    s.applyEvent(evt('subagent.started', { runId: 'rp', subRunId: 'sub1', parentToolCallId: 'sub1', description: '探索', prompt: '查实体' }));
    s.applyEvent(evt('agent.tool.invoked', { runId: 'rp', parentToolCallId: 'sub1', name: 'read_file', args: { path: 'x' }, toolCallId: 'c2' }));
    s.applyEvent(evt('agent.token.stream.delta', { runId: 'rp', parentToolCallId: 'sub1', delta: '子文' }));
    s.applyEvent(evt('subagent.completed', { runId: 'rp', subRunId: 'sub1', summary: '查完了' }));
    const sub = useChatStore.getState().messages[0].blocks[0];
    expect(sub.kind).toBe('subagent');
    if (sub.kind !== 'subagent') return;
    expect(sub.prompt).toBe('查实体');
    expect(sub.status).toBe('done');
    expect(sub.summary).toBe('查完了');
    expect(sub.work.some((b) => b.kind === 'tool' && b.name === 'read_file')).toBe(true);
    expect(sub.work.some((b) => b.kind === 'text' && b.text === '子文')).toBe(true);
  });

  it('D-036 后台子代理:自带 runId 单开卡片,回执落地且不锁输入框', () => {
    const s = useChatStore.getState();
    // 派发轮:dispatch 是普通工具行(不是 subagent 块),结果只是「已受理」。
    s.applyEvent(evt('agent.started', { runId: 'run_p', model: 'mock' }));
    s.applyEvent(evt('agent.tool.invoked', { runId: 'run_p', name: 'dispatch', args: { prompt: '摆僵尸', description: '摆放僵尸' }, toolCallId: 'c1' }));
    s.applyEvent(evt('agent.tool.completed', { runId: 'run_p', name: 'dispatch', ok: true, toolCallId: 'c1', output: '已受理:「摆放僵尸」…' }));
    s.applyEvent(evt('agent.message', { runId: 'run_p', text: '已派 1 个子代理' }));
    s.applyEvent(evt('agent.completed', { runId: 'run_p', text: '已派 1 个子代理' }));
    expect(useChatStore.getState().activeRunId).toBeNull();
    const parent = useChatStore.getState().messages[0];
    expect(parent.blocks[0]).toMatchObject({ kind: 'tool', name: 'dispatch', status: 'done' });

    // 后台腿:parentRunId = 自己的后台 run，无 agent.started —— 新卡片 + 元数据补齐。
    s.applyEvent(evt('subagent.started', {
      parentRunId: 'run_bg', subRunId: 'run_bg', parentToolCallId: 'run_bg',
      description: '摆放僵尸', prompt: '摆 5 个僵尸', detached: true, dispatchedBy: 'run_p', model: 'mock',
    }));
    let msgs = useChatStore.getState().messages;
    expect(msgs).toHaveLength(2);
    const card = msgs[1];
    expect(card.runId).toBe('run_bg');
    expect(card.model).toBe('mock');
    expect(card.time).not.toBe('');
    const sub = card.blocks[0];
    expect(sub).toMatchObject({ kind: 'subagent', id: 'run_bg', label: '摆放僵尸', status: 'running', detachedRunId: 'run_bg' });
    // 关键:后台跑着也不占 activeRunId,用户能继续发指令。
    expect(useChatStore.getState().activeRunId).toBeNull();

    // 完成:块结态 + 回执正文进同一张卡。
    s.applyEvent(evt('subagent.completed', { parentRunId: 'run_bg', subRunId: 'run_bg', summary: '已摆 5 个', detached: true }));
    s.applyEvent(evt('agent.message', { runId: 'run_bg', text: '子代理回执 · 摆放僵尸\n\n已摆 5 个', detached: true }));
    s.applyEvent(evt('agent.completed', { runId: 'run_bg', text: '子代理回执 · 摆放僵尸\n\n已摆 5 个', detached: true }));
    msgs = useChatStore.getState().messages;
    const done = msgs[1];
    expect(done.status).toBe('completed');
    expect(done.blocks[0]).toMatchObject({ kind: 'subagent', status: 'done', summary: '已摆 5 个' });
    expect(done.blocks.some((b) => b.kind === 'text' && b.text.includes('已摆 5 个'))).toBe(true);
    expect(useChatStore.getState().activeRunId).toBeNull();
  });

  it('D-038 回执唤醒轮:composer.user.message source=receipt → 用户卡标 source,照常挂助手卡并锁/解锁 activeRunId', () => {
    const s = useChatStore.getState();
    s.applyEvent(evt('composer.user.message', {
      text: '【系统唤醒】后台子代理回执送达(1 条)…', composerMode: 'multitask', runId: 'run_wake',
      source: 'receipt', receiptIds: ['rcpt_1'],
    }));
    s.applyEvent(evt('agent.started', { runId: 'run_wake', model: 'mock' }));
    let msgs = useChatStore.getState().messages;
    expect(msgs[0]).toMatchObject({ role: 'user', runId: 'run_wake', source: 'receipt', mode: 'multitask' });
    expect(msgs[1]).toMatchObject({ role: 'assistant', runId: 'run_wake', status: 'streaming' });
    // 唤醒轮是完整 turn:主 agent 在工作,输入框按常规锁住。
    expect(useChatStore.getState().activeRunId).toBe('run_wake');
    s.applyEvent(evt('agent.message', { runId: 'run_wake', text: '两项都完成了,总结如下…' }));
    s.applyEvent(evt('agent.completed', { runId: 'run_wake', text: '两项都完成了,总结如下…' }));
    msgs = useChatStore.getState().messages;
    expect(msgs[1].status).toBe('completed');
    expect(useChatStore.getState().activeRunId).toBeNull();
    // 普通用户消息不带 source。
    s.applyEvent(evt('composer.user.message', { text: '继续', composerMode: 'build', runId: 'run_u' }));
    const plain = useChatStore.getState().messages.find((m) => m.runId === 'run_u');
    expect(plain?.source).toBeUndefined();
  });

  it('D-038 SESSION_BUSY 409:撤掉乐观回显 + warning toast(不留幽灵用户卡)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: unknown) => {
        if (String(_url).includes('ask:execute')) {
          return {
            ok: false,
            status: 409,
            json: async () => ({ error: { code: 'SESSION_BUSY', message: '会话已有运行中的 run(run_wake)' } }),
            text: async () => '',
          } as Response;
        }
        return { ok: true, status: 200, json: async () => ({}), text: async () => '{}' } as Response;
      }),
    );
    await useChatStore.getState().sendMessage('抢发', 'build');
    expect(useChatStore.getState().messages.some((m) => m.role === 'user' && m.text === '抢发')).toBe(false);
    const toast = useToastStore.getState().items.find((t) => t.title.includes('后台回执'));
    expect(toast).toBeTruthy();
    expect(toast?.kind).toBe('warning');
    // D-047:后端没收下,「等待开轮」随之撤掉
    expect(useChatStore.getState().pendingTurnSince).toBeNull();
  });

  it('D-047 pendingTurnSince:发出即记、开轮(agent.started)清;开轮前即失败 / 发送失败同样清', async () => {
    await useChatStore.getState().sendMessage('你好', 'build');
    const since = useChatStore.getState().pendingTurnSince;
    expect(typeof since).toBe('number');
    const s = useChatStore.getState();
    s.applyEvent(evt('composer.user.message', { text: '你好', composerMode: 'build', runId: 'run_p' }));
    expect(useChatStore.getState().pendingTurnSince).toBe(since);
    s.applyEvent(evt('agent.started', { runId: 'run_p' }));
    expect(useChatStore.getState().pendingTurnSince).toBeNull();

    // 开轮前即失败(没有 agent.started)
    await useChatStore.getState().sendMessage('再来', 'build');
    expect(useChatStore.getState().pendingTurnSince).not.toBeNull();
    useChatStore.getState().applyEvent(evt('agent.failed', { runId: 'run_q', error: '模型未配置' }));
    expect(useChatStore.getState().pendingTurnSince).toBeNull();

    // 后端 5xx:乐观卡如实留着(既有语义),但不再挂「Planning next moves」
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => ({
        ok: false,
        status: 500,
        json: async () => ({ error: { code: 'INTERNAL', message: 'boom' } }),
        text: async () => '',
      }) as Response),
    );
    await useChatStore.getState().sendMessage('第三条', 'build');
    expect(useChatStore.getState().pendingTurnSince).toBeNull();
  });

  it('D-047 事件流连接态:订阅 connecting → 开流 live → 出错 down(连续失败不刷新起点)→ 重连复位;reset 回 idle', async () => {
    await useChatStore.getState().selectSession('sess_1');
    expect(useChatStore.getState().streamLink).toBe('connecting');
    sseCb?.onOpen?.();
    expect(useChatStore.getState()).toMatchObject({ streamLink: 'live', streamDownSince: null });
    const now = vi.spyOn(Date, 'now').mockReturnValueOnce(1_000).mockReturnValueOnce(5_000);
    try {
      sseCb?.onError?.(new Error('down'));
      expect(useChatStore.getState()).toMatchObject({ streamLink: 'down', streamDownSince: 1_000 });
      sseCb?.onError?.(new Error('still down'));
      expect(useChatStore.getState().streamDownSince).toBe(1_000);
    } finally {
      now.mockRestore();
    }
    sseCb?.onOpen?.();
    expect(useChatStore.getState()).toMatchObject({ streamLink: 'live', streamDownSince: null });
    useChatStore.getState().reset();
    expect(useChatStore.getState()).toMatchObject({ streamLink: 'idle', streamDownSince: null, pendingTurnSince: null });
  });

  it('D-036 cancelSubagent:打后台 run 自己的 cancel 端点', async () => {
    const fetchSpy = vi.mocked(fetch);
    await useChatStore.getState().cancelSubagent('run_bg');
    const call = fetchSpy.mock.calls.find((c) => String(c[0]).includes('/runs/run_bg/cancel'));
    expect(call).toBeTruthy();
    expect((call?.[1] as { method?: string })?.method).toBe('POST');
  });

  it('审批端点 HTTP 200 但 ok:false 时如实提示已失效', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ ok: false }),
    }) as Response));
    await useChatStore.getState().resolvePermission('expired-permission', true);
    expect(
      useToastStore.getState().items.some((item) => item.title.includes('已失效或已被处理')),
    ).toBe(true);
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
  it('selectSession:首次快照失败后短间隔重试；同会话并发调用复用请求并最终订阅', async () => {
    vi.useFakeTimers();
    const snapshot = {
      sessions: [],
      activeSession: { id: 'sess_1', selectedModelId: 'model-recovered' },
      events: [],
      todos: [],
      run: null,
      models: {
        models: [{ id: 'model-recovered', label: 'Recovered', provider: 'mock' }],
        defaultModelId: 'model-recovered',
      },
      latestSeq: 13,
      chatFolders: [],
    };
    const response = (ok: boolean, body: unknown, status = 200) => ({
      ok,
      status,
      json: async () => body,
    }) as Response;
    let snapshotAttempts = 0;
    vi.stubGlobal('fetch', vi.fn(async (url: unknown) => {
      const value = String(url);
      if (value.includes('/design-snapshot?sessionId=sess_1')) {
        snapshotAttempts += 1;
        if (snapshotAttempts === 1) {
          return response(false, { error: { code: 'TEMPORARY', message: 'not ready' } }, 503);
        }
        return response(true, snapshot);
      }
      if (value.includes('/sessions/sess_1/goal')) {
        return response(true, { goal: null, engine: 'local' });
      }
      throw new Error(`未 mock: ${value}`);
    }));

    try {
      const first = useChatStore.getState().selectSession('sess_1');
      const duplicate = useChatStore.getState().selectSession('sess_1');
      expect(duplicate).toBe(first);

      await vi.advanceTimersByTimeAsync(0);
      expect(snapshotAttempts).toBe(1);
      expect(useChatStore.getState()).toMatchObject({
        currentSessionId: null,
        hydrating: true,
      });
      expect(subscribeMock).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(SESSION_SNAPSHOT_RETRY_MS - 1);
      expect(snapshotAttempts).toBe(1);
      await vi.advanceTimersByTimeAsync(1);
      await first;

      expect(snapshotAttempts).toBe(2);
      expect(useChatStore.getState()).toMatchObject({
        currentSessionId: 'sess_1',
        hydrating: false,
        latestSeq: 13,
        selectedModelId: 'model-recovered',
      });
      expect(subscribeMock).toHaveBeenCalledTimes(1);
      expect(subscribeMock).toHaveBeenCalledWith('sess_1', 13, expect.anything());

      // ChatColumn 若先于 Composer 完成同一会话的选择，Composer 的显式选择也应幂等，
      // 不能再拉快照/重建 SSE（更不能 reset 掉随后产生的乐观消息）。
      await useChatStore.getState().selectSession('sess_1');
      expect(snapshotAttempts).toBe(2);
      expect(subscribeMock).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

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

  it('resync 旧快照晚到时不回盖新会话，也不关掉新 SSE', async () => {
    const snapshot = (id: string, model: string, latestSeq: number) => ({
      sessions: [],
      activeSession: { id, selectedModelId: model },
      events: [],
      todos: [{ id: `todo-${id}`, title: id, status: 'queued' }],
      run: null,
      models: {
        models: [{ id: model, label: model, provider: 'mock', availability: 'available' }],
        defaultModelId: model,
      },
      latestSeq,
      chatFolders: [],
    });
    const response = (body: unknown) => ({
      ok: true,
      status: 200,
      json: async () => body,
    }) as Response;
    let sess1Snapshots = 0;
    let releaseStale!: (value: Response) => void;
    const staleResponse = new Promise<Response>((resolve) => {
      releaseStale = resolve;
    });
    vi.stubGlobal('fetch', vi.fn(async (url: unknown) => {
      const value = String(url);
      if (value.includes('/goal')) return response({ goal: null, engine: 'local' });
      if (value.includes('sessionId=sess_1')) {
        sess1Snapshots += 1;
        if (sess1Snapshots === 1) return response(snapshot('sess_1', 'model-old', 11));
        return staleResponse;
      }
      if (value.includes('sessionId=sess_2')) {
        return response(snapshot('sess_2', 'model-new', 22));
      }
      throw new Error(`未 mock: ${value}`);
    }));

    useSessionStore.setState({ activeSessionId: 'sess_1' });
    await useChatStore.getState().selectSession('sess_1');
    const staleResync = useChatStore.getState().resync();

    useSessionStore.setState({ activeSessionId: 'sess_2' });
    await useChatStore.getState().selectSession('sess_2');
    releaseStale(response(snapshot('sess_1', 'model-stale', 99)));
    await staleResync;

    const state = useChatStore.getState();
    expect(state.currentSessionId).toBe('sess_2');
    expect(state.selectedModelId).toBe('model-new');
    expect(state.latestSeq).toBe(22);
    expect(subscribeMock.mock.calls.map(([id, fromSeq]) => [id, fromSeq])).toEqual([
      ['sess_1', 11],
      ['sess_2', 22],
    ]);
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

  it('cancelRun 返回 ok:false 时重拉快照清理过期运行态', async () => {
    const resync = vi.fn(async () => undefined);
    useChatStore.setState({ activeRunId: 'run_done', resync });
    vi.stubGlobal('fetch', vi.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ ok: false, status: 'completed' }),
    }) as Response));
    await useChatStore.getState().cancelRun();
    expect(resync).toHaveBeenCalledOnce();
    expect(useToastStore.getState().items.some((item) => item.title.includes('状态已同步'))).toBe(true);
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
          id: 'sess_1', title: 't', status: 'idle', agentKind: 'coding', agentEngine: 'local', selectedModelId: null,
          thinkingEnabled: false, reasoningEffort: null, contextOptionId: null,
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

  it('pickModel(null):wire 用空串清除服务端选择，不被 serde 当成字段缺省', async () => {
    useChatStore.setState({ selectedModelId: 'codex:gpt-5.6-sol' });
    await useChatStore.getState().pickModel(null);
    const call = vi.mocked(fetch).mock.calls.find((entry) =>
      String(entry[0]) === '/api/forge/sessions/sess_1' &&
      JSON.parse(String((entry[1] as { body: string }).body)).selectedModelId === '',
    );
    expect(call).toBeTruthy();
    expect(useChatStore.getState().selectedModelId).toBeNull();
  });
});

/**
 * D-044 UltraPlan:ultraplan.* 关口事件建卡(回放与实时同路径)、提交事件回填、
 * 回放不碰 ultraPlanStore / 不打网络、实时才重拉、ULTRAPLAN_* 4xx 撤乐观卡。
 * 后端未实现,事件与响应按 wire 契约手工构造。
 */
describe('chatStore UltraPlan(D-044)', () => {
  const UP = 'up_a1';
  const ULTRA_URL = '/api/forge/sessions/sess_1/ultraplan';
  const questionnaire = {
    title: '塔防问卷',
    understanding: '## 理解',
    sections: [{ id: 's1', title: '玩法', questions: [{ id: 'q1', kind: 'text', question: '主题?' }] }],
  };

  const flowState = (patch: Record<string, unknown> = {}) => ({
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
  });

  const snapshotWith = (events: ForgeEventWire[], ultraplan: unknown) => ({
    sessions: [],
    activeSession: { id: 'sess_1', selectedModelId: 'mock', ultraplan },
    events,
    todos: [],
    run: null,
    models: {
      models: [{ id: 'mock', label: 'Mock provider', provider: 'mock', availability: 'available' }],
      defaultModelId: 'mock',
    },
    latestSeq: events.length,
    chatFolders: [],
  });

  /** 带 …/ultraplan 路由的假后端(默认桩的 /sessions/sess_1 前缀会把它吞成会话 PATCH 响应)。 */
  function ultraBackend(opts: { ultraplan?: unknown; snapshot?: unknown } = {}) {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend({}, {
        '/api/forge/design-snapshot': opts.snapshot ?? snapshotWith([], null),
        [ULTRA_URL]: { ultraplan: opts.ultraplan ?? null, demo: null },
        '/api/forge/sessions/sess_1/ask:execute': { run: { id: 'run_1', status: 'completed' } },
        '/api/forge/sessions/sess_1/goal': { goal: null, engine: 'local' },
      }),
    );
  }

  const ultraGets = () =>
    vi.mocked(fetch).mock.calls.filter((c) => String(c[0]).split('?')[0] === ULTRA_URL);

  const assistantOf = (runId: string | null) =>
    useChatStore.getState().messages.find((m) => m.role === 'assistant' && m.runId === runId);

  const ultraBlocksOf = (runId: string | null) =>
    (assistantOf(runId)?.blocks ?? []).filter((b) => b.kind === 'ultraplan');

  const answersSubmitted = { runId: 'run_s', id: UP, rev: 1, answers: { q1: { text: '科幻' } }, delegated: 0 };
  const demoDecision = { runId: 'run_p', id: UP, iteration: 1, decision: 'approve' };
  const acceptanceRecorded = { id: UP, round: 1, results: [{ id: 'm1', status: 'pass' }], failed: [] };

  /** 一条完整流程:需求 → 问卷 → Demo → 计划 → 制作 → 验收 → 完成。 */
  function flowEvents(): ForgeEventWire[] {
    return [
      evt('composer.user.message', { text: '做一个塔防', composerMode: 'ultraplan', runId: 'run_d' }),
      evt('agent.started', { runId: 'run_d', model: 'mock' }),
      evt('ultraplan.started', { runId: 'run_d', id: UP, slug: 'td-a1b2', dir: '.forge/ultraplan/td-a1b2', title: '塔防' }),
      evt('ultraplan.stage', {
        runId: 'run_d', id: UP, stage: 'discovery', phase: 'running', running: 'discovery',
        effort: 'high', thinkingForced: true,
      }),
      evt('ultraplan.notice', { runId: 'run_d', id: UP, code: 'EXPLORE_SKIPPED', message: '项目为空,跳过调研' }),
      evt('ultraplan.context.injected', { runId: 'run_d', id: UP, kind: 'discovery', sections: [] }),
      evt('ultraplan.questionnaire', {
        runId: 'run_d', id: UP, rev: 1, path: '.forge/ultraplan/td-a1b2/questionnaire.json', questionnaire,
      }),
      evt('ultraplan.stage', { runId: 'run_d', id: UP, stage: 'questionnaire', phase: 'waiting', running: null }),
      evt('agent.completed', { runId: 'run_d', text: '' }),
      // 提交问卷 → 需求文档 + Demo
      evt('composer.user.message', {
        text: '已提交问卷(1 题)', composerMode: 'ultraplan', runId: 'run_s',
        ultraplan: { id: UP, action: 'answer', rev: 1 },
      }),
      evt('agent.started', { runId: 'run_s' }),
      evt('ultraplan.answers.submitted', answersSubmitted),
      evt('ultraplan.spec.written', { runId: 'run_s', id: UP, path: '.forge/ultraplan/td-a1b2/spec.md', chars: 1200 }),
      evt('ultraplan.demo.ready', {
        runId: 'run_s', id: UP, iteration: 1, entry: 'index.html', verified: true, probe: { ok: true, errors: [] },
      }),
      evt('agent.completed', { runId: 'run_s', text: '' }),
      // 通过 Demo → 计划
      evt('composer.user.message', {
        text: '通过 Demo,开始写计划', composerMode: 'ultraplan', runId: 'run_p',
        ultraplan: { id: UP, action: 'approve_demo', rev: 1 },
      }),
      evt('agent.started', { runId: 'run_p' }),
      evt('ultraplan.demo.decision', demoDecision),
      evt('ultraplan.plan.ready', {
        runId: 'run_p', id: UP, rev: 1, planPath: '.forge/plans/td-a1b2.plan.md', name: '塔防 MVP',
        overview: '三波敌人', gameMode: '2d', taskCount: 6, roles: ['scene-builder'], automated: 2, manual: 1,
      }),
      evt('agent.completed', { runId: 'run_p', text: '' }),
      // 开始制作 → 验收清单
      evt('composer.user.message', {
        text: '确认计划,开始制作', composerMode: 'team', runId: 'run_x',
        ultraplan: { id: UP, action: 'start_production', rev: 1 },
      }),
      evt('agent.started', { runId: 'run_x' }),
      evt('ultraplan.production.started', { runId: 'run_x', id: UP, phase: 'start', todoCount: 6 }),
      evt('ultraplan.acceptance.ready', {
        runId: 'run_x', id: UP, round: 1, manual: [{ id: 'm1', title: '手感', steps: '玩一局', expected: '不卡顿' }],
      }),
      evt('agent.completed', { runId: 'run_x', text: '制作完成' }),
      // REST:验收全过(两条都不带 runId)
      evt('ultraplan.acceptance.recorded', acceptanceRecorded),
      evt('ultraplan.done', { id: UP }),
    ];
  }

  /** 流程事件应建出的卡(回放与实时同一套断言)。 */
  function expectFlowCards() {
    const st = useChatStore.getState();
    expect(st.messages.map((m) => `${m.role}:${m.runId}`)).toEqual([
      'user:run_d', 'assistant:run_d',
      'user:run_s', 'assistant:run_s',
      'user:run_p', 'assistant:run_p',
      'user:run_x', 'assistant:run_x',
    ]);
    // 需求轮:只有问卷卡(started / stage / notice / context.injected 不进时间线),且已回填提交内容
    expect(assistantOf('run_d')?.blocks).toEqual([
      {
        kind: 'ultraplan',
        step: 'questionnaire',
        upId: UP,
        rev: 1,
        payload: expect.objectContaining({ rev: 1, questionnaire }),
        submitted: answersSubmitted,
      },
    ]);
    // Demo 轮:spec.written 不建卡;Demo 卡带上下一轮的「通过」决定
    expect(assistantOf('run_s')?.blocks).toEqual([
      {
        kind: 'ultraplan',
        step: 'demo',
        upId: UP,
        rev: 1,
        payload: expect.objectContaining({ iteration: 1, verified: true, entry: 'index.html' }),
        submitted: demoDecision,
      },
    ]);
    // 计划轮:计划卡尚未有「提交」事件
    expect(assistantOf('run_p')?.blocks).toEqual([
      {
        kind: 'ultraplan',
        step: 'plan',
        upId: UP,
        rev: 1,
        payload: expect.objectContaining({ planPath: '.forge/plans/td-a1b2.plan.md', taskCount: 6 }),
      },
    ]);
    // 制作轮:验收卡(已记录结果)+ 无 runId 的 done 卡接在同一条消息末尾
    expect(ultraBlocksOf('run_x')).toEqual([
      {
        kind: 'ultraplan',
        step: 'acceptance',
        upId: UP,
        rev: 1,
        payload: expect.objectContaining({ round: 1 }),
        submitted: acceptanceRecorded,
      },
      { kind: 'ultraplan', step: 'done', upId: UP, rev: 0, payload: { id: UP } },
    ]);
    expect(assistantOf('run_x')?.blocks.map((b) => b.kind)).toEqual(['ultraplan', 'text', 'ultraplan']);
    // 卡片动作的用户回显带 ultraAction;普通自由文本不带
    const users = st.messages.filter((m) => m.role === 'user');
    expect(users.map((m) => m.ultraAction)).toEqual([
      undefined,
      { id: UP, action: 'answer', rev: 1 },
      { id: UP, action: 'approve_demo', rev: 1 },
      { id: UP, action: 'start_production', rev: 1 },
    ]);
    expect(users[1].text).toBe('已提交问卷(1 题)');
  }

  it('实时:关口事件在对应助手消息上建卡,提交事件回填 submitted,并触发阶段重拉', async () => {
    ultraBackend({ ultraplan: flowState({ stage: 'done', acceptanceRound: 1 }) });
    const s = useChatStore.getState();
    for (const e of flowEvents()) s.applyEvent(e);
    expectFlowCards();
    // 实时 ultraplan.* → GET …/ultraplan(一串事件合并,不是每条一个请求)
    expect(ultraGets().length).toBeGreaterThanOrEqual(1);
    await vi.waitFor(() => {
      expect(useUltraPlanStore.getState().state).toMatchObject({ id: UP, stage: 'done' });
    });
    expect(ultraGets().length).toBeLessThanOrEqual(2);
    // 实时 notice 进 store(仅实时)
    expect(useUltraPlanStore.getState().notices).toEqual([
      { id: UP, runId: 'run_d', code: 'EXPLORE_SKIPPED', message: '项目为空,跳过调研' },
    ]);
  });

  it('回放:只建卡——不改 ultraPlanStore、不打 …/ultraplan', async () => {
    ultraBackend({ snapshot: snapshotWith(flowEvents(), null) });
    await useChatStore.getState().selectSession('sess_1');
    expectFlowCards();
    // 快照里会话没有流程(已「重新开始」):历史事件不得把阶段重新走出来
    expect(useUltraPlanStore.getState().state).toBeNull();
    expect(useUltraPlanStore.getState().notices).toEqual([]);
    expect(useUltraPlanStore.getState().live).toBeNull();
    expect(ultraGets()).toHaveLength(0);
  });

  it('回放:阶段取快照 activeSession.ultraplan;有流程只补拉一次,回放事件本身不通知 store', async () => {
    const refresh = vi.fn(async () => undefined);
    const applyLiveEvent = vi.fn();
    const noteUserMessage = vi.fn();
    useUltraPlanStore.setState({ refresh, applyLiveEvent, noteUserMessage });
    ultraBackend({ snapshot: snapshotWith(flowEvents(), flowState({ stage: 'done', acceptanceRound: 1 })) });
    await useChatStore.getState().selectSession('sess_1');
    expectFlowCards();
    expect(useUltraPlanStore.getState().state).toMatchObject({ id: UP, stage: 'done', acceptanceRound: 1 });
    expect(useUltraPlanStore.getState().sessionId).toBe('sess_1');
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(refresh).toHaveBeenCalledWith('sess_1');
    expect(applyLiveEvent).not.toHaveBeenCalled();
    expect(noteUserMessage).not.toHaveBeenCalled();
    // 订阅建立后到达的实时事件照常通知
    const live = evt('ultraplan.stage', { id: UP, stage: 'done', phase: 'waiting', running: null });
    sseCb?.onEvent({ id: String(live.seq), event: live.type, data: live });
    expect(applyLiveEvent).toHaveBeenCalledTimes(1);
    expect(applyLiveEvent).toHaveBeenCalledWith(expect.objectContaining({ type: 'ultraplan.stage' }));
    const echo = evt('composer.user.message', {
      text: '继续制作', composerMode: 'team', runId: 'run_r', ultraplan: { id: UP, action: 'resume_production' },
    });
    sseCb?.onEvent({ id: String(echo.seq), event: echo.type, data: echo });
    expect(noteUserMessage).toHaveBeenCalledWith({ id: UP, action: 'resume_production', rev: null });
  });

  it('无 runId 的 REST 事件:回退出的新 Demo 挂到最后一条助手消息;旧卡保留', () => {
    ultraBackend();
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'run_s' }));
    s.applyEvent(evt('ultraplan.demo.ready', {
      runId: 'run_s', id: UP, iteration: 2, entry: 'index.html', verified: true, probe: { ok: true, errors: [] },
    }));
    s.applyEvent(evt('agent.completed', { runId: 'run_s', text: '' }));
    s.applyEvent(evt('ultraplan.demo.ready', {
      id: UP, iteration: 3, entry: 'index.html', verified: false, note: '已回退到第 1 版(未重新验证)',
      probe: { ok: false, errors: [], unavailable: true }, rollbackOf: 1,
    }));
    const st = useChatStore.getState();
    expect(st.messages).toHaveLength(1);
    expect(st.messages[0].status).toBe('completed');
    expect(ultraBlocksOf('run_s')).toMatchObject([
      { step: 'demo', rev: 2 },
      { step: 'demo', rev: 3, payload: { rollbackOf: 1, verified: false } },
    ]);
  });

  it('无 runId 且会话里还没有助手消息:单开一条已完成的独立消息(不留 streaming 空卡)', () => {
    ultraBackend();
    useChatStore.getState().applyEvent(evt('ultraplan.done', { id: UP }, 'evt_done'));
    const st = useChatStore.getState();
    expect(st.messages).toHaveLength(1);
    expect(st.messages[0]).toMatchObject({
      id: 'ultraplan-evt_done',
      role: 'assistant',
      runId: null,
      status: 'completed',
      time: '10:24',
      blocks: [{ kind: 'ultraplan', step: 'done', upId: UP, rev: 0 }],
    });
    expect(st.activeRunId).toBeNull();
  });

  it('同一 (upId, step, rev) 重复到达不出第二张卡;缺 id 的事件忽略;找不到卡的提交事件不报错', () => {
    ultraBackend();
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'run_d' }));
    s.applyEvent(evt('ultraplan.questionnaire', { runId: 'run_d', id: UP, rev: 1, questionnaire }));
    s.applyEvent(evt('ultraplan.answers.submitted', { ...answersSubmitted, runId: 'run_d' }));
    // 重投(新事件 id,同一问卷版本):换载荷、保留已回填的 submitted
    s.applyEvent(evt('ultraplan.questionnaire', {
      runId: 'run_d', id: UP, rev: 1, questionnaire: { ...questionnaire, title: '塔防问卷(重投)' },
    }));
    expect(ultraBlocksOf('run_d')).toHaveLength(1);
    expect(ultraBlocksOf('run_d')[0]).toMatchObject({
      payload: { questionnaire: { title: '塔防问卷(重投)' } },
      submitted: { answers: { q1: { text: '科幻' } } },
    });
    // 重出的问卷(rev 2)是另一张卡,旧卡的 submitted 不串过去
    s.applyEvent(evt('ultraplan.questionnaire', { runId: 'run_d', id: UP, rev: 2, questionnaire }));
    expect(ultraBlocksOf('run_d')).toHaveLength(2);
    expect(ultraBlocksOf('run_d')[1]).not.toHaveProperty('submitted');
    const before = useChatStore.getState().messages;
    s.applyEvent(evt('ultraplan.questionnaire', { runId: 'run_d', rev: 3, questionnaire }));
    s.applyEvent(evt('ultraplan.demo.decision', { runId: 'run_d', id: UP, iteration: 9, decision: 'revise', feedback: 'x' }));
    expect(useChatStore.getState().messages.flatMap((m) => m.blocks)).toEqual(before.flatMap((m) => m.blocks));
  });

  it('实时 composer.user.message 回显 → ultraPlanStore.act 的在途动作判定为已受理', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown) => {
        // ask:execute 要到整轮结束才回:这里永不返回,受理只能靠回显事件
        if (String(url).includes('ask:execute')) return new Promise<Response>(() => undefined);
        return {
          ok: true,
          status: 200,
          json: async () => ({ ultraplan: flowState({ stage: 'demo_review', demoIteration: 1 }) }),
        } as Response;
      }),
    );
    useUltraPlanStore.getState().hydrate(flowState({ stage: 'demo_review', demoIteration: 1 }));
    const pending = useUltraPlanStore.getState().approveDemo();
    expect(useUltraPlanStore.getState().pending).toEqual({ action: 'approve_demo', rev: 1 });
    useChatStore.getState().applyEvent(evt('composer.user.message', {
      text: '通过 Demo,开始写计划', composerMode: 'ultraplan', runId: 'run_p',
      ultraplan: { id: UP, action: 'approve_demo', rev: 1 },
    }));
    expect(useUltraPlanStore.getState().pending).toBeNull();
    await expect(pending).resolves.toEqual({ ok: true });
    expect(useChatStore.getState().messages[0]).toMatchObject({
      role: 'user',
      text: '通过 Demo,开始写计划',
      mode: 'ultraplan',
      ultraAction: { id: UP, action: 'approve_demo', rev: 1 },
    });
  });

  it('409 ULTRAPLAN_STAGE_MISMATCH:撤掉乐观回显 + warning toast(后端原因)+ 重拉阶段', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown) => {
        if (String(url).includes('ask:execute')) {
          return {
            ok: false,
            status: 409,
            json: async () => ({
              error: {
                code: 'ULTRAPLAN_STAGE_MISMATCH',
                message: '制作进行中,请用「继续制作」',
                details: { stage: 'production', allowed: ['resume_production'] },
              },
            }),
          } as Response;
        }
        return {
          ok: true,
          status: 200,
          json: async () => ({ ultraplan: flowState({ stage: 'production', phase: 'failed' }) }),
        } as Response;
      }),
    );
    useUltraPlanStore.getState().hydrate(flowState({ stage: 'plan_review', planRev: 1 }));
    await useChatStore.getState().sendMessage('再加一个 Boss', 'ultraplan');
    expect(useChatStore.getState().messages.some((m) => m.role === 'user' && m.text === '再加一个 Boss')).toBe(false);
    const toast = useToastStore.getState().items.find((t) => t.title === '制作进行中,请用「继续制作」');
    expect(toast?.kind).toBe('warning');
    expect(useToastStore.getState().items.some((t) => t.title.includes('提交失败'))).toBe(false);
    // 被拒说明本地阶段过期:planning 复位,阶段重拉
    expect(usePlanStore.getState().planning).toBe(false);
    expect(ultraGets()).toHaveLength(1);
    await vi.waitFor(() => expect(useUltraPlanStore.getState().state?.stage).toBe('production'));
  });

  it('非 ULTRAPLAN_* 的 4xx 维持原行为(不在本次改动范围内)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => ({
        ok: false,
        status: 400,
        json: async () => ({ error: { code: 'INVALID_INPUT', message: 'mode 不受支持' } }),
      }) as Response),
    );
    await useChatStore.getState().sendMessage('做一个塔防', 'ultraplan');
    expect(useChatStore.getState().messages.some((m) => m.role === 'user' && m.text === '做一个塔防')).toBe(true);
    expect(useToastStore.getState().items.some((t) => t.title.includes('提交失败'))).toBe(true);
  });

  it('sendMessage:ultraplan 自由文本只发 {userInput, mode};仅计划关口点亮 planning,plan 模式不回退', async () => {
    ultraBackend();
    await useChatStore.getState().sendMessage('做一个塔防', 'ultraplan');
    const ask = vi.mocked(fetch).mock.calls.find((c) => String(c[0]).includes('ask:execute'));
    expect(JSON.parse(String((ask?.[1] as { body: string }).body))).toEqual({
      userInput: '做一个塔防',
      mode: 'ultraplan',
    });
    expect(usePlanStore.getState().planning).toBe(false);

    useUltraPlanStore.getState().hydrate(flowState({ stage: 'questionnaire' }));
    await useChatStore.getState().sendMessage('换成科幻题材', 'ultraplan');
    expect(usePlanStore.getState().planning).toBe(false);

    useUltraPlanStore.getState().hydrate(flowState({ stage: 'plan_review', planRev: 1 }));
    await useChatStore.getState().sendMessage('把 Boss 战提前', 'ultraplan');
    expect(usePlanStore.getState().planning).toBe(true);
    // 别的模式在计划关口发消息是普通 turn,不点亮
    usePlanStore.getState().setPlanning(false);
    await useChatStore.getState().sendMessage('随便聊聊', 'ask');
    expect(usePlanStore.getState().planning).toBe(false);
    await useChatStore.getState().sendMessage('调研一下波次系统', 'plan');
    expect(usePlanStore.getState().planning).toBe(true);
  });

  it('实时 session.updated:本会话有流程才重拉阶段', () => {
    ultraBackend();
    useSessionStore.setState({ loadAll: vi.fn(async () => undefined) });
    const s = useChatStore.getState();
    s.applyEvent(evt('session.updated', {}));
    expect(ultraGets()).toHaveLength(0);
    useUltraPlanStore.getState().hydrate(flowState());
    s.applyEvent(evt('session.updated', {}));
    expect(ultraGets()).toHaveLength(1);
  });

  it('reset():流程状态随会话清空(与计划指针同处)', () => {
    useUltraPlanStore.getState().hydrate(flowState());
    useUltraPlanStore.setState({ lastActionError: { code: 'X', message: 'y' } });
    useChatStore.getState().reset();
    expect(useUltraPlanStore.getState()).toMatchObject({ state: null, sessionId: null, lastActionError: null });
  });

  it('实时 ultraplan.demo.ready 开(或激活)Demo 页签(同 plan.created 开 Plan 页签);其余关口事件不开', () => {
    ultraBackend({ ultraplan: flowState({ stage: 'demo_review', demoIteration: 1 }) });
    useWorkbenchStore.setState({ tabs: [], activeTabId: null });
    useUltraPlanStore.getState().hydrate(flowState(), 'sess_1');
    const s = useChatStore.getState();
    s.applyEvent(evt('agent.started', { runId: 'run_d' }));
    s.applyEvent(evt('ultraplan.questionnaire', { runId: 'run_d', id: UP, rev: 1, questionnaire }));
    s.applyEvent(evt('agent.started', { runId: 'run_s' }));
    s.applyEvent(evt('ultraplan.answers.submitted', answersSubmitted));
    expect(useWorkbenchStore.getState().tabs).toEqual([]);

    s.applyEvent(evt('ultraplan.demo.ready', {
      runId: 'run_s', id: UP, iteration: 1, entry: 'index.html', verified: true, probe: { ok: true, errors: [] },
    }));
    expect(useWorkbenchStore.getState().activeTabId).toBe(`demo:${UP}`);
    expect(useWorkbenchStore.getState().tabs).toEqual([
      { id: `demo:${UP}`, kind: 'demo', title: 'Demo · 塔防', upId: UP, sessionId: 'sess_1' },
    ]);

    // 用户切到别的页签后来了新一版(回退 / 改版):同一页签被重新激活,不开第二个
    useWorkbenchStore.getState().openTab('todo');
    s.applyEvent(evt('ultraplan.demo.ready', {
      id: UP, iteration: 2, entry: 'index.html', verified: false, note: '已回退到第 1 版(未重新验证)',
      probe: { ok: false, errors: [] }, rollbackOf: 1,
    }));
    expect(useWorkbenchStore.getState().activeTabId).toBe(`demo:${UP}`);
    expect(useWorkbenchStore.getState().tabs.filter((t) => t.kind === 'demo')).toHaveLength(1);
  });

  it('回放:历史 ultraplan.demo.ready 只建卡,不开 Demo 页签', async () => {
    useWorkbenchStore.setState({ tabs: [], activeTabId: null });
    ultraBackend({ snapshot: snapshotWith(flowEvents(), flowState({ stage: 'done', acceptanceRound: 1 })) });
    await useChatStore.getState().selectSession('sess_1');
    expect(assistantOf('run_s')?.blocks).toEqual([
      expect.objectContaining({ kind: 'ultraplan', step: 'demo', rev: 1 }),
    ]);
    expect(useWorkbenchStore.getState().tabs.some((t) => t.kind === 'demo')).toBe(false);
    expect(useWorkbenchStore.getState().activeTabId).toBeNull();
  });
});
