import { apiClient, type ApiClient } from './client';
import type { AuthConfig, LoginRequest, LoginResponse, MeResponse, OkResponse } from './types';

/** §3.1 认证 + /me（管理后台只用到这几项）。 */
export function createAuthApi(client: ApiClient) {
  return {
    config: () => client.request<AuthConfig>('GET', '/api/v1/auth/config', { auth: false }),
    login: (input: LoginRequest) =>
      client.request<LoginResponse>('POST', '/api/v1/auth/login', { body: input, auth: false }),
    /** 吊销当前会话（走 Bearer + 401 刷新）。 */
    logout: () => client.request<OkResponse>('POST', '/api/v1/auth/logout'),
    /** 用指定 access token 吊销会话，不经过本地会话（登录成功但非管理员时用）。 */
    logoutWithToken: (accessToken: string) =>
      client.request<OkResponse>('POST', '/api/v1/auth/logout', {
        auth: false,
        headers: { Authorization: `Bearer ${accessToken}` },
      }),
    me: () => client.request<MeResponse>('GET', '/api/v1/me'),
  };
}

export type AuthApi = ReturnType<typeof createAuthApi>;

export const authApi = createAuthApi(apiClient);
