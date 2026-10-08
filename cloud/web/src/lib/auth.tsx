import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { createAuthApi, type AuthApi } from './api/auth';
import { ApiError, apiClient, type ApiClient } from './api/client';
import type { User } from './api/types';
import { adminDeviceInfo } from './device';

export type AuthStatus = 'loading' | 'anonymous' | 'authenticated';

export const NOT_ADMIN_MESSAGE = '该账号不是管理员，无法登录管理后台';

interface AuthState {
  status: AuthStatus;
  user: User | null;
}

interface AuthContextValue extends AuthState {
  authApi: AuthApi;
  login(email: string, password: string): Promise<User>;
  logout(): Promise<void>;
}

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({ client = apiClient, children }: { client?: ApiClient; children: ReactNode }) {
  const authApi = useMemo(() => createAuthApi(client), [client]);
  const [state, setState] = useState<AuthState>(() => ({
    status: client.getAccessToken() || client.getRefreshToken() ? 'loading' : 'anonymous',
    user: null,
  }));

  // 刷新失败（refresh token 失效/被吊销）→ 回到未登录，RequireAuth 负责跳登录页。
  useEffect(() => client.onAuthFailure(() => setState({ status: 'anonymous', user: null })), [client]);

  // 启动时用 localStorage 里的 refresh token 恢复会话。
  useEffect(() => {
    if (state.status !== 'loading') return;
    let cancelled = false;
    (async () => {
      try {
        const ok = client.getAccessToken() ? true : await client.refresh();
        if (!ok) {
          client.clearSession();
          throw new Error('refresh token invalid');
        }
        const me = await authApi.me();
        if (me.user.role !== 'admin') {
          await authApi.logout().catch(() => undefined);
          client.clearSession();
          throw new Error('not admin');
        }
        if (!cancelled) setState({ status: 'authenticated', user: me.user });
      } catch {
        // 网络/5xx 时 refresh() 会抛错但不清会话：保留 token，下次打开仍会尝试恢复。
        if (!cancelled) setState({ status: 'anonymous', user: null });
      }
    })();
    return () => {
      cancelled = true;
    };
    // 仅挂载时执行一次
  }, []);

  const login = useCallback(
    async (email: string, password: string) => {
      const res = await authApi.login({
        email: email.trim(),
        password,
        device: adminDeviceInfo(),
        issueDeviceKey: false,
      });
      if (res.user.role !== 'admin') {
        // 服务端已为该账号建立会话：立刻吊销，不在本地保存任何令牌。
        await authApi.logoutWithToken(res.accessToken).catch(() => undefined);
        throw new ApiError(403, 'NOT_ADMIN', NOT_ADMIN_MESSAGE);
      }
      client.setSession(res);
      setState({ status: 'authenticated', user: res.user });
      return res.user;
    },
    [authApi, client],
  );

  const logout = useCallback(async () => {
    try {
      if (client.getAccessToken() || client.getRefreshToken()) await authApi.logout();
    } catch {
      // 服务端失败也照常清本地会话
    }
    client.clearSession();
    setState({ status: 'anonymous', user: null });
  }, [authApi, client]);

  const value = useMemo<AuthContextValue>(
    () => ({ ...state, authApi, login, logout }),
    [state, authApi, login, logout],
  );
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error('useAuth 必须在 <AuthProvider> 内使用');
  return ctx;
}
