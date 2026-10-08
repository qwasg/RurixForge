import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { beforeEach, describe, expect, it } from 'vitest';
import { ApiClient, REFRESH_TOKEN_KEY } from '@/lib/api/client';
import type { Role } from '@/lib/api/types';
import { AuthProvider, NOT_ADMIN_MESSAGE } from '@/lib/auth';
import { DEVICE_ID_KEY } from '@/lib/device';
import { LoginPage } from '@/pages/LoginPage';
import { json, memoryStorage, mockFetch } from './helpers';

function loginResponse(role: Role) {
  return {
    accessToken: `at-${role}`,
    accessExpiresAt: '2026-09-26T10:15:00Z',
    refreshToken: `rt-${role}`,
    refreshExpiresAt: '2026-10-26T10:00:00Z',
    user: {
      id: 7,
      email: 'someone@example.com',
      nickname: '',
      role,
      status: 'active',
      hasAvatar: false,
      avatarVersion: 0,
      groupId: 1,
      groupName: 'default',
      balanceMicros: 0,
      createdAt: '2026-09-01T00:00:00Z',
    },
    deviceKey: null,
  };
}

function renderLogin(role: Role) {
  const storage = memoryStorage();
  const { fn, calls } = mockFetch((c) => {
    if (c.url === '/api/v1/auth/config') {
      return json(200, { registrationMode: 'open', requireEmailVerify: false, smtpEnabled: false, siteName: 'Test Cloud', currency: 'USD' });
    }
    if (c.url === '/api/v1/auth/login') return json(200, loginResponse(role));
    if (c.url === '/api/v1/auth/logout') return json(200, { ok: true });
    return undefined;
  });
  const client = new ApiClient({ fetch: fn, storage, crossTabLock: false });
  render(
    <AuthProvider client={client}>
      <MemoryRouter initialEntries={['/login']}>
        <Routes>
          <Route path="/login" element={<LoginPage />} />
          <Route path="/" element={<p>控制台首页</p>} />
        </Routes>
      </MemoryRouter>
    </AuthProvider>,
  );
  return { client, storage, calls };
}

function submit(email: string, password: string) {
  fireEvent.change(screen.getByLabelText('邮箱'), { target: { value: email } });
  fireEvent.change(screen.getByLabelText('密码'), { target: { value: password } });
  fireEvent.click(screen.getByRole('button', { name: '登录' }));
}

describe('LoginPage', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('rejects non-admin users and revokes the session the login just created', async () => {
    const { client, storage, calls } = renderLogin('user');
    expect(await screen.findByText('Test Cloud')).toBeInTheDocument();

    submit(' someone@example.com ', 'hunter22');

    expect(await screen.findByRole('alert')).toHaveTextContent(NOT_ADMIN_MESSAGE);
    const login = calls.find((c) => c.url === '/api/v1/auth/login');
    expect(login?.method).toBe('POST');
    expect(login?.headers.authorization).toBeUndefined();
    expect(login?.body).toEqual({
      email: 'someone@example.com',
      password: 'hunter22',
      device: { id: localStorage.getItem(DEVICE_ID_KEY), name: '管理后台', platform: 'web', appVersion: 'admin' },
      issueDeviceKey: false,
    });
    expect(localStorage.getItem(DEVICE_ID_KEY)).toBeTruthy();

    await waitFor(() => expect(calls.some((c) => c.url === '/api/v1/auth/logout')).toBe(true));
    expect(calls.find((c) => c.url === '/api/v1/auth/logout')?.headers.authorization).toBe('Bearer at-user');
    expect(client.getAccessToken()).toBeNull();
    expect(storage.map.has(REFRESH_TOKEN_KEY)).toBe(false);
    expect(screen.queryByText('控制台首页')).not.toBeInTheDocument();
  });

  it('keeps the session and enters the console for admins', async () => {
    const { client, storage, calls } = renderLogin('admin');

    submit('admin@example.com', 'hunter22');

    expect(await screen.findByText('控制台首页')).toBeInTheDocument();
    expect(client.getAccessToken()).toBe('at-admin');
    expect(storage.map.get(REFRESH_TOKEN_KEY)).toBe('rt-admin');
    expect(calls.some((c) => c.url === '/api/v1/auth/logout')).toBe(false);
  });
});
