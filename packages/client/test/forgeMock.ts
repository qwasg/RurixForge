import { vi } from 'vitest';

/** 按工具名路由的假后端:返回 agentd 信封形态(content[0].text 内嵌 JSON)。
 * 工具名含前缀(mcp__engine-scene__/mcp__asset-pipeline__),key 存前缀后的名字。
 * rest:非 MCP 的 /api/forge/* REST 面(F3 swarm/skills 管理),key = 完整路径,
 * value 为响应体或 (init) => 响应体(GET 无 body 亦可命中)。
 */
export function mockForgeBackend(
  map: Record<string, unknown>,
  rest: Record<string, unknown> = {},
) {
  return vi.fn(async (_url: unknown, init?: { body?: string }) => {
    const url = String(_url);
    if (url in rest) {
      const v = rest[url];
      return {
        ok: true,
        status: 200,
        json: async () => (typeof v === 'function' ? (v as (i?: { body?: string }) => unknown)(init) : v),
      } as Response;
    }
    const { tool } = JSON.parse(init?.body ?? '{}') as { tool: string };
    const name = tool
      .replace('mcp__engine-scene__', '')
      .replace('mcp__asset-pipeline__', '')
      .replace('mcp__code-forge__', '')
      .replace('mcp__gen-image__', '');
    if (!(name in map)) throw new Error(`未 mock 的工具: ${name}(full=${tool})`);
    return {
      ok: true,
      status: 200,
      json: async () => ({ content: [{ type: 'text', text: JSON.stringify(map[name]) }] }),
    } as Response;
  });
}

/**
 * 增强 mock 工厂:可动态改写返回值;支持资产管线工具。
 */
export class ForgeMock {
  private _map: Record<string, unknown> = {};
  private _fetch = vi.fn(async (_url: unknown, init?: { body?: string }) => {
    const { tool } = JSON.parse(init?.body ?? '{}') as { tool: string };
    const name = tool
      .replace('mcp__engine-scene__', '')
      .replace('mcp__asset-pipeline__', '')
      .replace('mcp__code-forge__', '')
      .replace('mcp__gen-image__', '');
    if (!(name in this._map)) throw new Error(`未 mock 的工具: ${name}(full=${tool})`);
    return {
      ok: true,
      status: 200,
      json: async () => ({ content: [{ type: 'text', text: JSON.stringify(this._map[name]) }] }),
    } as Response;
  });

  reset() {
    this._map = {};
    this._fetch.mockClear();
  }

  get fetch() {
    return this._fetch;
  }

  setAssets(assets: unknown[]) {
    this._map['asset_list'] = { assets };
  }

  setBuildStatus(items: unknown[]) {
    this._map['asset_build_status'] = { items };
  }

  setEntityList(entities: unknown[]) {
    this._map['entity_list'] = { entities };
  }

  setDefault(name: string, value: unknown) {
    this._map[name] = value;
  }

  stubGlobal() {
    vi.stubGlobal('fetch', this._fetch);
  }

  get calls() {
    return this._fetch.mock.calls.map((c) => {
      const init = c[1] as { body: string };
      return JSON.parse(init.body) as { tool: string; arguments: Record<string, unknown> };
    });
  }
}

export const forgeMock = new ForgeMock();
