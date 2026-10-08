import { vi } from 'vitest';

export interface Call {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: unknown;
}

export function json(status: number, body: unknown): Response {
  return new Response(body === undefined ? null : JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

export type Route = (call: Call) => Response | Promise<Response> | undefined;

/** 按顺序匹配路由的 fetch 桩；记录每次调用（URL、方法、头、JSON 体）。 */
export function mockFetch(route: Route) {
  const calls: Call[] = [];
  const fn = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const headers: Record<string, string> = {};
    new Headers(init?.headers).forEach((v, k) => {
      headers[k] = v;
    });
    const call: Call = {
      url: typeof input === 'string' ? input : input instanceof URL ? input.href : input.url,
      method: (init?.method ?? 'GET').toUpperCase(),
      headers,
      body: typeof init?.body === 'string' ? JSON.parse(init.body) : undefined,
    };
    calls.push(call);
    const res = await route(call);
    if (!res) throw new Error(`unexpected request: ${call.method} ${call.url}`);
    return res;
  });
  return { fn: fn as unknown as typeof fetch, calls };
}

export function memoryStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    map,
  };
}
