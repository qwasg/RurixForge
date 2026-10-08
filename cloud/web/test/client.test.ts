import { describe, expect, it, vi } from 'vitest';
import { ApiClient, ApiError, REFRESH_TOKEN_KEY } from '@/lib/api/client';
import { json, memoryStorage, mockFetch } from './helpers';

const unauthorized = () => json(401, { error: { code: 'UNAUTHORIZED', message: '未登录' } });

describe('ApiClient: 401 → 单飞刷新 → 重试', () => {
  it('refreshes once for concurrent 401s and retries every request with the new token', async () => {
    const storage = memoryStorage();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { fn, calls } = mockFetch(async (c) => {
      if (c.url === '/api/v1/auth/refresh') {
        await gate;
        return json(200, {
          accessToken: 'at-new',
          accessExpiresAt: '2026-09-26T10:15:00Z',
          refreshToken: 'rt-new',
          refreshExpiresAt: '2026-10-26T10:00:00Z',
        });
      }
      return c.headers.authorization === 'Bearer at-new' ? json(200, { path: c.url }) : unauthorized();
    });
    const client = new ApiClient({ fetch: fn, storage, crossTabLock: false });
    client.setSession({ accessToken: 'at-old', refreshToken: 'rt-old' });

    const all = Promise.all(['/api/admin/a', '/api/admin/b', '/api/admin/c'].map((p) => client.request('GET', p)));
    const refreshCalls = () => calls.filter((c) => c.url === '/api/v1/auth/refresh');
    // 三个请求都已拿到 401 并在等待同一次刷新
    await vi.waitFor(() => {
      expect(calls.filter((c) => c.url.startsWith('/api/admin/'))).toHaveLength(3);
      expect(refreshCalls()).toHaveLength(1);
    });
    release();

    await expect(all).resolves.toEqual([{ path: '/api/admin/a' }, { path: '/api/admin/b' }, { path: '/api/admin/c' }]);
    expect(refreshCalls()).toHaveLength(1);
    expect(refreshCalls()[0]).toMatchObject({ method: 'POST', body: { refreshToken: 'rt-old' } });
    expect(refreshCalls()[0].headers.authorization).toBeUndefined();
    expect(calls.filter((c) => c.url.startsWith('/api/admin/'))).toHaveLength(6);
    expect(client.getAccessToken()).toBe('at-new');
    expect(storage.map.get(REFRESH_TOKEN_KEY)).toBe('rt-new');
  });

  it('clears the session and notifies listeners when the refresh token is rejected', async () => {
    const storage = memoryStorage();
    const { fn, calls } = mockFetch((c) =>
      c.url === '/api/v1/auth/refresh'
        ? json(401, { error: { code: 'REFRESH_REUSED', message: '会话已失效' } })
        : unauthorized(),
    );
    const client = new ApiClient({ fetch: fn, storage, crossTabLock: false });
    client.setSession({ accessToken: 'at-old', refreshToken: 'rt-old' });
    const onFailure = vi.fn();
    client.onAuthFailure(onFailure);

    const results = await Promise.allSettled([client.request('GET', '/api/admin/a'), client.request('GET', '/api/admin/b')]);

    for (const r of results) {
      expect(r.status).toBe('rejected');
      const err = (r as PromiseRejectedResult).reason as ApiError;
      expect(err).toBeInstanceOf(ApiError);
      expect(err.status).toBe(401);
      expect(err.code).toBe('SESSION_EXPIRED');
    }
    expect(calls.filter((c) => c.url === '/api/v1/auth/refresh')).toHaveLength(1);
    expect(onFailure).toHaveBeenCalled();
    expect(client.getAccessToken()).toBeNull();
    expect(storage.map.has(REFRESH_TOKEN_KEY)).toBe(false);
  });

  it('surfaces error.message from non-401 errors without refreshing', async () => {
    const { fn, calls } = mockFetch(() => json(409, { error: { code: 'GROUP_IN_USE', message: '分组仍被用户或套餐引用' } }));
    const client = new ApiClient({ fetch: fn, storage: memoryStorage(), crossTabLock: false });
    client.setSession({ accessToken: 'at', refreshToken: 'rt' });

    await expect(client.request('DELETE', '/api/admin/groups/3')).rejects.toMatchObject({
      status: 409,
      code: 'GROUP_IN_USE',
      message: '分组仍被用户或套餐引用',
    });
    expect(calls).toHaveLength(1);
    expect(calls[0].headers.authorization).toBe('Bearer at');
  });
});
