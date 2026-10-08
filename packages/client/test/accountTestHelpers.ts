import type { AccountStatus } from '@/lib/accountApi';

/** 默认云账户 status 夹具(§8.2 登录门与 BYO 折叠测试共用)。 */
export function makeAccountStatus(over: Partial<AccountStatus> = {}): AccountStatus {
  return {
    serverUrl: 'http://127.0.0.1:8110',
    loggedIn: false,
    reachable: true,
    user: null,
    balanceMicros: 0,
    currency: 'USD',
    subscriptions: [],
    deviceKeyPrefix: null,
    byoAllowed: true,
    byoConfigured: false,
    devMock: false,
    sync: {
      settings: true,
      memory: true,
      skills: true,
      lastSyncAt: null,
      lastError: null,
    },
    lastError: null,
    ...over,
  };
}

export function jsonResponse(body: unknown, ok = true, status = 200): Response {
  return { ok, status, json: async () => body } as Response;
}

export function accountStatusResponse(over: Partial<AccountStatus> = {}): Response {
  return jsonResponse(makeAccountStatus(over));
}

/** 在 vitest fetch mock 链最前调用:处理 GET /api/forge/account/status。 */
export function tryAccountStatusFetch(url: unknown, init?: { method?: string }, over?: Partial<AccountStatus>): Response | null {
  const path = String(url).split('?')[0];
  const method = init?.method ?? 'GET';
  if (method === 'GET' && path === '/api/forge/account/status') {
    return accountStatusResponse(over);
  }
  return null;
}
