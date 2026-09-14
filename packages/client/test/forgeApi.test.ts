import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  callTool,
  deleteGoal,
  ForgeApiError,
  getCodexAccount,
  getCodexMcpStatus,
  getCodexModels,
  getCodexRateLimits,
  getCodexStatus,
  getGoal,
  postCodexConfig,
  postCodexInstall,
  postCodexLogin,
  postCodexLogout,
  postGoalPause,
  postGoalResume,
  putGoal,
  unwrapToolResult,
} from '@/lib/forgeApi';

/** 构造 agentd 形态的 fetch 响应(content[0].text 内嵌 JSON 字符串) */
function okResponse(envelope: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => envelope,
  } as Response;
}

function envelopeOf(value: unknown, isError = false) {
  return {
    content: [{ type: 'text', text: JSON.stringify(value) }],
    ...(isError ? { isError: true } : {}),
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('unwrapToolResult', () => {
  it('content[0].text 二次解析为对象', () => {
    expect(unwrapToolResult(envelopeOf({ a: 1 }))).toEqual({ a: 1 });
  });

  it('text 非 JSON 时回传原文', () => {
    expect(unwrapToolResult({ content: [{ type: 'text', text: 'plain text' }] })).toBe(
      'plain text',
    );
  });

  it('无 content 时退化 structuredContent,再退化本体', () => {
    expect(unwrapToolResult({ structuredContent: { b: 2 } })).toEqual({ b: 2 });
    expect(unwrapToolResult({ c: 3 })).toEqual({ c: 3 });
  });
});

describe('callTool', () => {
  it('成功:POST mcp/call,工具名补前缀,返回二次解析结果', async () => {
    const fetchMock = vi.fn(async () => okResponse(envelopeOf({ entities: [] })));
    vi.stubGlobal('fetch', fetchMock);

    const r = await callTool<{ entities: unknown[] }>('entity_list');
    expect(r).toEqual({ entities: [] });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, { body: string }];
    expect(url).toBe('/api/forge/mcp/call');
    const body = JSON.parse(init.body) as { tool: string; arguments: unknown; workspaceId?: string };
    expect(body.tool).toBe('mcp__engine-scene__entity_list');
    expect(body.arguments).toEqual({});
    expect(body.workspaceId).toBeUndefined();
  });

  it('当前工作区已选 → 请求体带 workspaceId(agentd 按工作区项目根路由 engine-host)', async () => {
    localStorage.setItem('forge:activeWorkspace', 'ws_pvz');
    try {
      const fetchMock = vi.fn(async () => okResponse(envelopeOf({ state: 'edit' })));
      vi.stubGlobal('fetch', fetchMock);
      await callTool('play_state');
      const [, init] = fetchMock.mock.calls[0] as unknown as [string, { body: string }];
      expect(JSON.parse(init.body).workspaceId).toBe('ws_pvz');
    } finally {
      localStorage.removeItem('forge:activeWorkspace');
    }
  });

  it('信封 isError → 抛 TOOL_ERROR(取内嵌 message)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => okResponse(envelopeOf({ message: '实体 9 不存在' }, true))),
    );
    const err = await callTool('entity_get', { id: 9 }).catch((e) => e);
    expect(err).toBeInstanceOf(ForgeApiError);
    expect((err as ForgeApiError).code).toBe('TOOL_ERROR');
    expect((err as ForgeApiError).message).toBe('实体 9 不存在');
  });

  it('HTTP 非 2xx → 抛结构化 code(如 UPSTREAM_UNREACHABLE)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        okResponse({ error: { code: 'UPSTREAM_UNREACHABLE', message: 'agentd down' } }, 502),
      ),
    );
    const err = await callTool('scene_summary').catch((e) => e);
    expect(err).toBeInstanceOf(ForgeApiError);
    expect((err as ForgeApiError).code).toBe('UPSTREAM_UNREACHABLE');
    expect((err as ForgeApiError).status).toBe(502);
  });

  it('fetch 本身抛错 → NETWORK', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new Error('socket hangup');
      }),
    );
    const err = await callTool('host_ping').catch((e) => e);
    expect((err as ForgeApiError).code).toBe('NETWORK');
  });
});

describe('Codex / Goal REST wrappers', () => {
  it('方法、路径、query 与请求体保持后端契约', async () => {
    const calls: Array<{ url: string; method: string; body?: unknown }> = [];
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
      const method = init?.method ?? 'GET';
      const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined;
      calls.push({ url: String(url), method, body });
      const value = String(url).includes('/goal')
        ? String(url).endsWith('/goal') && method === 'DELETE'
          ? { ok: true }
          : { goal: { objective: 'ship', status: 'active', engine: 'codex' } }
        : String(url).endsWith('/models?refresh=true')
          ? { ok: true, models: [] }
          : String(url).endsWith('/rate-limits')
            ? { ok: true, rateLimits: null }
            : String(url).endsWith('/mcp/status?sessionId=sess%20one')
              ? {
                  autoRegisterMcp: true,
                  projectRoot: 'D:/repo',
                  servers: [],
                  live: true,
                  runtime: {
                    data: [{
                      name: 'engine-scene',
                      runtimeStatus: 'connected',
                      pluginId: null,
                      serverInfo: null,
                      tools: {},
                      resources: [],
                      resourceTemplates: [],
                      authStatus: 'unsupported',
                    }],
                    nextCursor: null,
                  },
                }
              : { ok: true };
      return {
        ok: true,
        status: 200,
        json: async () => value,
        text: async () => JSON.stringify(value),
      } as Response;
    }));

    await getCodexStatus('sess one');
    await postCodexInstall();
    await postCodexConfig({ defaultEngine: 'codex', computerUse: true });
    await postCodexLogin({ kind: 'apiKey', apiKey: 'secret' });
    await postCodexLogout();
    await getCodexAccount();
    await getCodexModels(true);
    await getCodexRateLimits();
    const mcp = await getCodexMcpStatus('sess one');
    await getGoal('sess one');
    await putGoal('sess one', { objective: 'ship', tokenBudget: 123 });
    await postGoalPause('sess one');
    await postGoalResume('sess one');
    await deleteGoal('sess one');

    expect(mcp).toMatchObject({
      live: true,
      runtime: {
        data: [{ name: 'engine-scene', runtimeStatus: 'connected', authStatus: 'unsupported' }],
        nextCursor: null,
      },
    });

    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      'GET /api/forge/codex/status?sessionId=sess%20one',
      'POST /api/forge/codex/install',
      'POST /api/forge/codex/config',
      'POST /api/forge/codex/login',
      'POST /api/forge/codex/logout',
      'GET /api/forge/codex/account',
      'GET /api/forge/codex/models?refresh=true',
      'GET /api/forge/codex/rate-limits',
      'GET /api/forge/codex/mcp/status?sessionId=sess%20one',
      'GET /api/forge/sessions/sess%20one/goal',
      'PUT /api/forge/sessions/sess%20one/goal',
      'POST /api/forge/sessions/sess%20one/goal/pause',
      'POST /api/forge/sessions/sess%20one/goal/resume',
      'DELETE /api/forge/sessions/sess%20one/goal',
    ]);
    expect(calls[2].body).toEqual({ defaultEngine: 'codex', computerUse: true });
    expect(calls[3].body).toEqual({ kind: 'apiKey', apiKey: 'secret' });
    expect(calls[10].body).toEqual({ objective: 'ship', tokenBudget: 123 });
  });
});
