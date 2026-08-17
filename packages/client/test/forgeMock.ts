import { vi } from 'vitest';

/** 按工具名路由的假后端:返回 agentd 信封形态(content[0].text 内嵌 JSON) */
export function mockForgeBackend(map: Record<string, unknown>) {
  return vi.fn(async (_url: unknown, init?: { body?: string }) => {
    const { tool } = JSON.parse(init?.body ?? '{}') as { tool: string };
    const name = tool.replace('mcp__engine-scene__', '');
    if (!(name in map)) throw new Error(`未 mock 的工具: ${name}`);
    return {
      ok: true,
      status: 200,
      json: async () => ({ content: [{ type: 'text', text: JSON.stringify(map[name]) }] }),
    } as Response;
  });
}
