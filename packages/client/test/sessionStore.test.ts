import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useSessionStore, type ForgeSession } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';

/**
 * F7 wave.3 sessionStore:fetch mock 下 CRUD / 乐观更新 / 失败回滚 / fork。
 * wire 对齐 agentd sessions.rs:GET {sessions} / POST {session} / PATCH {session} /
 * DELETE 空体 / POST fork {session};folders GET {folders} / POST {folder}。
 */

function makeSession(id: string, over: Partial<ForgeSession> = {}): ForgeSession {
  return {
    id,
    title: `会话 ${id}`,
    status: 'idle',
    agentKind: 'coding',
    agentEngine: 'local',
    selectedModelId: null,
    thinkingEnabled: false,
    reasoningEffort: null,
    contextOptionId: null,
    webSearchEnabled: true,
    activeRunId: null,
    createdAt: '2026-08-18T00:00:00Z',
    updatedAt: '2026-08-18T00:00:00Z',
    pinned: false,
    titleManuallySet: false,
    folderId: null,
    ...over,
  };
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

/** 方法感知的 REST 桩;failOn 命中的方法+路径子串 → 500 失败(回滚断言用) */
function stubRest(handlers: Record<string, unknown>, failOn?: { method: string; path: string }) {
  const calls: Call[] = [];
  const fn = vi.fn(async (url: unknown, init?: { method?: string; body?: string }) => {
    const u = String(url);
    const method = init?.method ?? 'GET';
    const body = init?.body ? JSON.parse(init.body) : undefined;
    calls.push({ method, url: u, body });
    if (failOn && method === failOn.method && u.includes(failOn.path)) {
      return {
        ok: false,
        status: 500,
        json: async () => ({ error: { code: 'BOOM', message: '模拟失败' } }),
        text: async () => JSON.stringify({ error: { code: 'BOOM', message: '模拟失败' } }),
      } as Response;
    }
    for (const [key, v] of Object.entries(handlers)) {
      const [m, p] = key.split(' ');
      if (m === method && u.includes(p)) {
        const payload = typeof v === 'function' ? (v as (b: unknown) => unknown)(body) : v;
        return {
          ok: true,
          status: 200,
          json: async () => payload,
          text: async () => JSON.stringify(payload),
        } as Response;
      }
    }
    return {
      ok: false,
      status: 404,
      json: async () => ({ error: { code: 'NOT_FOUND', message: `未 stub: ${method} ${u}` } }),
      text: async () => JSON.stringify({ error: { code: 'NOT_FOUND', message: 'nf' } }),
    } as Response;
  });
  vi.stubGlobal('fetch', fn);
  return calls;
}

const initial = useSessionStore.getState();

beforeEach(() => {
  useSessionStore.setState(initial, true);
  useToastStore.getState().clear();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('sessionStore', () => {
  it('loadAll:GET sessions + chat-folders 填充', async () => {
    stubRest({
      'GET /api/forge/sessions': { sessions: [makeSession('sess_a')] },
      'GET /api/forge/chat-folders': { folders: [{ id: 'f1', name: '工作', createdAt: '', updatedAt: '' }] },
      'GET /api/forge/design-snapshot': { agents: { defaultEngine: 'codex' } },
    });
    await useSessionStore.getState().loadAll();
    const s = useSessionStore.getState();
    expect(s.sessions).toHaveLength(1);
    expect(s.sessions[0].id).toBe('sess_a');
    expect(s.folders).toHaveLength(1);
    expect(s.offline).toBe(false);
    expect(s.defaultAgentEngine).toBe('codex');
    expect(s.draftAgentEngine).toBe('codex');
  });

  it('loadAll 失败:offline=true + toast 报错', async () => {
    stubRest({}, { method: 'GET', path: '/api/forge/sessions' });
    await useSessionStore.getState().loadAll();
    expect(useSessionStore.getState().offline).toBe(true);
    expect(useToastStore.getState().items.some((t) => t.kind === 'error')).toBe(true);
  });

  it('create:POST /sessions → 列表置顶 + 选中', async () => {
    const created = makeSession('sess_new', { title: '' });
    const calls = stubRest({
      'POST /api/forge/sessions': { session: created },
    });
    useSessionStore.setState({ sessions: [makeSession('sess_old')] });
    const r = await useSessionStore.getState().create();
    expect(r?.id).toBe('sess_new');
    const s = useSessionStore.getState();
    expect(s.sessions[0].id).toBe('sess_new');
    expect(s.activeSessionId).toBe('sess_new');
    expect(calls[0].method).toBe('POST');
  });

  it('create:把 Composer 规格带进 POST,供无会话直发继承 thinking', async () => {
    const created = makeSession('sess_spec', { thinkingEnabled: true, reasoningEffort: 'max' });
    const calls = stubRest({
      'POST /api/forge/sessions': { session: created },
    });
    await useSessionStore.getState().create(undefined, {
      thinkingEnabled: true,
      reasoningEffort: 'max',
      contextOptionId: '1m',
    });
    expect(calls[0].body).toEqual({
      title: '',
      agentEngine: 'local',
      thinkingEnabled: true,
      reasoningEffort: 'max',
      contextOptionId: '1m',
    });
  });

  it('主页草稿引擎进入创建载荷；显式 spec 优先', async () => {
    const calls = stubRest({
      'POST /api/forge/sessions': (body: unknown) => ({
        session: makeSession('sess_engine', {
          agentEngine: (body as { agentEngine: 'local' | 'codex' }).agentEngine,
        }),
      }),
    });
    useSessionStore.getState().setDraftAgentEngine('codex');
    await useSessionStore.getState().create();
    expect(calls[0].body).toMatchObject({ agentEngine: 'codex' });

    await useSessionStore.getState().create(undefined, { agentEngine: 'local' });
    expect(calls[1].body).toMatchObject({ agentEngine: 'local' });
  });

  it('setAgentEngine:乐观 PATCH，成功采用后端模型选择；失败回滚', async () => {
    const prev = makeSession('sess_a', { agentEngine: 'local', selectedModelId: 'deepseek-chat' });
    const calls = stubRest({
      'PATCH /api/forge/sessions/': (body: unknown) => ({
        session: {
          ...prev,
          agentEngine: (body as { agentEngine: 'local' | 'codex' }).agentEngine,
          selectedModelId: 'codex:gpt-5.6-terra',
        },
      }),
    });
    useSessionStore.setState({ sessions: [prev], activeSessionId: 'sess_a' });
    await useSessionStore.getState().setAgentEngine('sess_a', 'codex');
    expect(calls[0].body).toEqual({ agentEngine: 'codex' });
    expect(useSessionStore.getState().sessions[0]).toMatchObject({
      agentEngine: 'codex',
      selectedModelId: 'codex:gpt-5.6-terra',
    });

    useSessionStore.setState({ sessions: [prev], activeSessionId: 'sess_a' });
    vi.unstubAllGlobals();
    stubRest({}, { method: 'PATCH', path: '/api/forge/sessions/' });
    await useSessionStore.getState().setAgentEngine('sess_a', 'codex');
    expect(useSessionStore.getState().sessions[0].agentEngine).toBe('local');
  });

  it('rename:乐观改名 + PATCH title;失败回滚 + toast', async () => {
    const prev = makeSession('sess_a', { title: '旧名' });
    const calls = stubRest({
      'PATCH /api/forge/sessions/': (b: unknown) => ({
        session: { ...prev, title: (b as { title: string }).title, titleManuallySet: true },
      }),
    });
    useSessionStore.setState({ sessions: [prev] });
    await useSessionStore.getState().rename('sess_a', '新名');
    expect(useSessionStore.getState().sessions[0].title).toBe('新名');
    const patch = calls.find((c) => c.method === 'PATCH');
    expect((patch?.body as { title: string }).title).toBe('新名');

    // 失败回滚
    useSessionStore.setState({ sessions: [prev] });
    vi.unstubAllGlobals();
    stubRest({}, { method: 'PATCH', path: '/api/forge/sessions/' });
    await useSessionStore.getState().rename('sess_a', '会失败');
    expect(useSessionStore.getState().sessions[0].title).toBe('旧名');
    expect(useToastStore.getState().items.some((t) => t.kind === 'error')).toBe(true);
  });

  it('togglePin:PATCH pinned 乐观翻转', async () => {
    const prev = makeSession('sess_a');
    stubRest({
      'PATCH /api/forge/sessions/': (b: unknown) => ({
        session: { ...prev, pinned: (b as { pinned: boolean }).pinned },
      }),
    });
    useSessionStore.setState({ sessions: [prev] });
    await useSessionStore.getState().togglePin('sess_a');
    expect(useSessionStore.getState().sessions[0].pinned).toBe(true);
  });

  it('remove:乐观移除 + 清空 active;失败回滚', async () => {
    const prev = makeSession('sess_a');
    stubRest({ 'DELETE /api/forge/sessions/': null });
    useSessionStore.setState({ sessions: [prev], activeSessionId: 'sess_a' });
    await useSessionStore.getState().remove('sess_a');
    expect(useSessionStore.getState().sessions).toHaveLength(0);
    expect(useSessionStore.getState().activeSessionId).toBeNull();

    useSessionStore.setState({ sessions: [prev], activeSessionId: 'sess_a' });
    vi.unstubAllGlobals();
    stubRest({}, { method: 'DELETE', path: '/api/forge/sessions/' });
    await useSessionStore.getState().remove('sess_a');
    expect(useSessionStore.getState().sessions).toHaveLength(1);
    expect(useSessionStore.getState().activeSessionId).toBe('sess_a');
  });

  it('moveToFolder:PATCH folderId(三态 null 清除)', async () => {
    const prev = makeSession('sess_a', { folderId: 'f1' });
    const calls = stubRest({
      'PATCH /api/forge/sessions/': (b: unknown) => ({
        session: { ...prev, folderId: (b as { folderId: string | null }).folderId },
      }),
    });
    useSessionStore.setState({ sessions: [prev] });
    await useSessionStore.getState().moveToFolder('sess_a', null);
    expect(useSessionStore.getState().sessions[0].folderId).toBeNull();
    const patch = calls.find((c) => c.method === 'PATCH');
    expect(patch?.body).toEqual({ folderId: null });
  });

  it('fork:POST :fork → 新分支置顶并选中', async () => {
    const src = makeSession('sess_a', { title: '主会话' });
    const forked = makeSession('sess_b', { title: '分支 · 主会话' });
    const calls = stubRest({ 'POST /api/forge/sessions/sess_a/fork': { session: forked } });
    useSessionStore.setState({ sessions: [src], activeSessionId: 'sess_a' });
    const r = await useSessionStore.getState().fork('sess_a');
    expect(r?.id).toBe('sess_b');
    const s = useSessionStore.getState();
    expect(s.sessions[0].id).toBe('sess_b');
    expect(s.activeSessionId).toBe('sess_b');
    expect(calls[0].url).toContain('/api/forge/sessions/sess_a/fork');
  });

  it('createFolder / removeFolder(级联清 folderId 乐观同步)', async () => {
    const calls = stubRest({
      'POST /api/forge/chat-folders': (b: unknown) => ({
        folder: { id: 'f9', name: (b as { name: string }).name, createdAt: '', updatedAt: '' },
      }),
      'DELETE /api/forge/chat-folders/': null,
    });
    const f = await useSessionStore.getState().createFolder(' 项目 A ');
    expect(f?.id).toBe('f9');
    expect((calls[0].body as { name: string }).name).toBe('项目 A'); // trim
    expect(useSessionStore.getState().folders).toHaveLength(1);

    useSessionStore.setState({ sessions: [makeSession('sess_a', { folderId: 'f9' })] });
    await useSessionStore.getState().removeFolder('f9');
    const s = useSessionStore.getState();
    expect(s.folders).toHaveLength(0);
    expect(s.sessions[0].folderId).toBeNull();
  });
});
