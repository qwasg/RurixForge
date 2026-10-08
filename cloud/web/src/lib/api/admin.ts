import { apiClient, type ApiClient, type Query } from './client';
import type {
  Account,
  AccountListParams,
  AccountTestResult,
  AdminModel,
  AdminOrder,
  AdminUser,
  AuditListParams,
  AuditLog,
  CreateApiKeyAccountInput,
  CreateRedeemInput,
  CreateRedeemResult,
  CreateUserInput,
  Dashboard,
  Group,
  GroupInput,
  ImportCodexInput,
  ImportCodexResult,
  ItemsResponse,
  ModelInput,
  OAuthExchangeInput,
  OAuthStartResult,
  OkResponse,
  OrderListParams,
  Page,
  PatchAccountInput,
  PatchUserInput,
  Plan,
  PlanInput,
  RedeemCode,
  RedeemListParams,
  Settings,
  SettingsInput,
  Subscription,
  UsageListParams,
  UsagePage,
  UserDetail,
  UserListParams,
} from './types';

const BASE = '/api/admin';

/** §7 / §11.3 管理 API。写接口的返回值只作即时回显，页面随后仍会重新拉列表。 */
export function createAdminApi(client: ApiClient) {
  const get = <T>(path: string, query?: Query) => client.request<T>('GET', BASE + path, { query });
  const post = <T>(path: string, body: unknown = {}) => client.request<T>('POST', BASE + path, { body });
  const patch = <T>(path: string, body: unknown) => client.request<T>('PATCH', BASE + path, { body });
  const put = <T>(path: string, body: unknown) => client.request<T>('PUT', BASE + path, { body });
  const del = <T = OkResponse>(path: string) => client.request<T>('DELETE', BASE + path);

  return {
    dashboard: () => get<Dashboard>('/dashboard'),

    users: {
      list: (params: UserListParams = {}) => get<Page<AdminUser>>('/users', params),
      create: (input: CreateUserInput) => post<AdminUser>('/users', input),
      get: (id: number) => get<UserDetail>(`/users/${id}`),
      update: (id: number, input: PatchUserInput) => patch<AdminUser>(`/users/${id}`, input),
      adjustBalance: (id: number, deltaMicros: number, note: string) =>
        post<{ balanceMicros: number }>(`/users/${id}/balance`, { deltaMicros, note }),
      setPassword: (id: number, password: string) => post<OkResponse>(`/users/${id}/password`, { password }),
      grantSubscription: (id: number, planId: number, days: number) =>
        post<Subscription>(`/users/${id}/subscriptions`, { planId, days }),
      cancelSubscription: (id: number, subId: number) => del(`/users/${id}/subscriptions/${subId}`),
    },

    groups: {
      list: () => get<ItemsResponse<Group>>('/groups'),
      create: (input: GroupInput) => post<Group>('/groups', input),
      update: (id: number, input: Partial<GroupInput>) => patch<Group>(`/groups/${id}`, input),
      remove: (id: number) => del(`/groups/${id}`),
    },

    plans: {
      list: () => get<ItemsResponse<Plan>>('/plans'),
      create: (input: PlanInput) => post<Plan>('/plans', input),
      update: (id: number, input: Partial<PlanInput>) => patch<Plan>(`/plans/${id}`, input),
      remove: (id: number) => del(`/plans/${id}`),
    },

    accounts: {
      list: (params: AccountListParams = {}) => get<Page<Account>>('/accounts', params),
      create: (input: CreateApiKeyAccountInput) => post<Account>('/accounts', input),
      update: (id: number, input: PatchAccountInput) => patch<Account>(`/accounts/${id}`, input),
      remove: (id: number) => del(`/accounts/${id}`),
      importCodex: (input: ImportCodexInput) => post<ImportCodexResult>('/accounts/import-codex', input),
      oauthStart: () => post<OAuthStartResult>('/accounts/oauth/openai/start', {}),
      oauthExchange: (input: OAuthExchangeInput) => post<Account>('/accounts/oauth/openai/exchange', input),
      refreshToken: (id: number) => post<Account>(`/accounts/${id}/refresh`),
      refreshQuota: (id: number) => post<Account>(`/accounts/${id}/quota`),
      clearCooldown: (id: number) => post<Account>(`/accounts/${id}/clear-cooldown`),
      test: (id: number, model: string) => post<AccountTestResult>(`/accounts/${id}/test`, { model }),
      models: (id: number) => get<ItemsResponse<string>>(`/accounts/${id}/models`),
    },

    models: {
      list: () => get<ItemsResponse<AdminModel>>('/models'),
      create: (input: ModelInput) => post<AdminModel>('/models', input),
      update: (id: string, input: Partial<ModelInput>) =>
        patch<AdminModel>(`/models/${encodeURIComponent(id)}`, input),
      remove: (id: string) => del(`/models/${encodeURIComponent(id)}`),
    },

    redeemCodes: {
      list: (params: RedeemListParams = {}) => get<Page<RedeemCode>>('/redeem-codes', params),
      create: (input: CreateRedeemInput) => post<CreateRedeemResult>('/redeem-codes', input),
      revoke: (id: number) => post<unknown>(`/redeem-codes/${id}/revoke`),
      exportCsv: (batch?: string) =>
        client.requestBlob('GET', `${BASE}/redeem-codes/export`, { query: { batch }, headers: { Accept: 'text/csv' } }),
    },

    /** §11.3 订单：非 pending 的标记已支付 / 取消 → 409 ORDER_NOT_PENDING。 */
    orders: {
      list: (params: OrderListParams = {}) => get<Page<AdminOrder>>('/orders', params),
      /** 线下确认收款；note 可选（≤ 200 字），为空时不传。 */
      markPaid: (id: number, note?: string) => post<AdminOrder>(`/orders/${id}/mark-paid`, note ? { note } : {}),
      cancel: (id: number) => post<AdminOrder>(`/orders/${id}/cancel`),
    },

    usage: {
      list: (params: UsageListParams = {}) => get<UsagePage>('/usage', params),
    },

    settings: {
      get: () => get<Settings>('/settings'),
      update: (input: SettingsInput) => put<Settings>('/settings', input),
    },

    auditLogs: {
      list: (params: AuditListParams = {}) => get<Page<AuditLog>>('/audit-logs', params),
    },
  };
}

export type AdminApi = ReturnType<typeof createAdminApi>;

export const adminApi = createAdminApi(apiClient);
