import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useGoalStore } from '@/lib/goalStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

interface Call {
  url: string;
  method: string;
  body?: unknown;
}

function response(value: unknown): Response {
  return {
    ok: true,
    status: 200,
    json: async () => value,
    text: async () => JSON.stringify(value),
  } as Response;
}

const baseGoal = {
  sessionId: 'sess_goal',
  objective: '完成 Codex 接入',
  status: 'active' as const,
  engine: 'local' as const,
  tokenBudget: 2000,
  tokensUsed: 20,
  timeUsedSeconds: 3,
  turns: 1,
  updatedAt: '2026-09-04T00:00:00Z',
};

const initialGoal = useGoalStore.getState();
const initialSessions = useSessionStore.getState();

beforeEach(() => {
  useGoalStore.setState(initialGoal, true);
  useGoalStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useSessionStore.setState({ activeSessionId: 'sess_goal' });
  useWorkbenchStore.setState({ tabs: [], activeTabId: null });
  useToastStore.getState().clear();
});

afterEach(() => vi.unstubAllGlobals());

describe('goalStore', () => {
  it('refreshGoal 读取当前会话且不会因加载历史目标抢焦点', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => response({ goal: baseGoal, engine: 'local' })));
    await useGoalStore.getState().refreshGoal();
    expect(useGoalStore.getState().goal).toMatchObject(baseGoal);
    expect(useGoalStore.getState().history).toHaveLength(1);
    expect(useWorkbenchStore.getState().tabs).toEqual([]);
  });

  it('set/pause/resume/clear 走统一 REST 并同步状态', async () => {
    const calls: Call[] = [];
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
      const method = init?.method ?? 'GET';
      const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined;
      calls.push({ url: String(url), method, body });
      if (method === 'PUT') return response({ goal: { ...baseGoal, objective: body.objective, tokenBudget: body.tokenBudget } });
      if (String(url).endsWith('/pause')) return response({ goal: { ...baseGoal, status: 'paused' } });
      if (String(url).endsWith('/resume')) return response({ goal: { ...baseGoal, status: 'active' } });
      return response({ ok: true });
    }));

    await useGoalStore.getState().setGoal('  交付前端  ', 4096);
    expect(calls[0]).toMatchObject({
      url: '/api/forge/sessions/sess_goal/goal', method: 'PUT',
      body: { objective: '交付前端', tokenBudget: 4096 },
    });
    expect(useGoalStore.getState().goal?.objective).toBe('交付前端');
    expect(useWorkbenchStore.getState().activeTabId).toBe('goal');

    await useGoalStore.getState().pauseGoal();
    expect(useGoalStore.getState().goal?.status).toBe('paused');
    await useGoalStore.getState().resumeGoal();
    expect(useGoalStore.getState().goal?.status).toBe('active');
    expect(await useGoalStore.getState().clearGoal()).toBe(true);
    expect(useGoalStore.getState().goal).toBeNull();
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      'PUT /api/forge/sessions/sess_goal/goal',
      'POST /api/forge/sessions/sess_goal/goal/pause',
      'POST /api/forge/sessions/sess_goal/goal/resume',
      'DELETE /api/forge/sessions/sess_goal/goal',
    ]);
  });

  it('goal.updated/cleared 归一 Codex 事件；回放不自动开页签', () => {
    const event = {
      sessionId: 'sess_goal',
      type: 'goal.updated',
      ts: '2026-09-04T01:00:00Z',
      payload: {
        engine: 'codex',
        goal: {
          objective: '持续修复', status: 'active', tokenBudget: 500, tokensUsed: 10,
          createdAt: 1_800_000_000, updatedAt: 1_800_000_000_000,
        },
      },
    };
    useGoalStore.getState().applyGoalEvent(event, true);
    expect(useGoalStore.getState().goal).toMatchObject({
      objective: '持续修复', engine: 'codex', tokenBudget: 500, tokensUsed: 10,
      createdAt: new Date(1_800_000_000_000).toISOString(),
      updatedAt: new Date(1_800_000_000_000).toISOString(),
    });
    expect(useWorkbenchStore.getState().tabs).toEqual([]);
    useGoalStore.getState().applyGoalEvent({
      sessionId: 'sess_goal', type: 'goal.cleared', ts: '2026-09-04T01:01:00Z', payload: {},
    });
    expect(useGoalStore.getState().goal).toBeNull();
    expect(useGoalStore.getState().history.at(-1)?.status).toBe('cleared');
  });

  it('写操作会使在途 refresh 失效，旧 GET 不回盖新目标', async () => {
    let finishGet: ((value: Response) => void) | undefined;
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
      if ((init?.method ?? 'GET') === 'GET') {
        return new Promise<Response>((resolve) => {
          finishGet = resolve;
        });
      }
      return response({ goal: { ...baseGoal, objective: '新目标', tokenBudget: 9000 } });
    }));
    const stale = useGoalStore.getState().refreshGoal();
    await vi.waitFor(() => expect(finishGet).toBeTypeOf('function'));
    await useGoalStore.getState().setGoal('新目标', 9000);
    finishGet?.(response({ goal: { ...baseGoal, objective: '旧目标' } }));
    await stale;
    expect(useGoalStore.getState().goal).toMatchObject({ objective: '新目标', tokenBudget: 9000 });
  });

  it('切换会话后忽略旧 goal SSE 与旧请求错误', async () => {
    useSessionStore.setState({ activeSessionId: 'sess_new' });
    useGoalStore.setState({
      sessionId: 'sess_new',
      goal: { ...baseGoal, sessionId: 'sess_new', objective: '新会话目标' },
      history: [],
      error: null,
    });
    useGoalStore.getState().applyGoalEvent({
      sessionId: 'sess_goal', type: 'goal.updated',
      payload: { engine: 'codex', goal: { objective: '旧会话目标', status: 'active' } },
    });
    expect(useGoalStore.getState().goal?.objective).toBe('新会话目标');

    let rejectWrite: ((reason: Error) => void) | undefined;
    vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>((_resolve, reject) => {
      rejectWrite = reject;
    })));
    const pending = useGoalStore.getState().setGoal('仍是新目标');
    await vi.waitFor(() => expect(rejectWrite).toBeTypeOf('function'));
    useSessionStore.setState({ activeSessionId: 'sess_third' });
    rejectWrite?.(new Error('旧请求失败'));
    await pending;
    expect(useGoalStore.getState().error).toBeNull();
    expect(useToastStore.getState().items).toEqual([]);
  });
});
