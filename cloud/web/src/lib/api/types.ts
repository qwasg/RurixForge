/**
 * forge-cloud 接口类型（15_CLOUD_SERVICE.md §3.1、§3.2、§7、§11）。
 * 约定：camelCase、时间 RFC3339、金额整数 micros（字段名以 Micros 结尾）。
 */

export type Role = 'user' | 'admin';
export type UserStatus = 'active' | 'disabled';
export type Platform = 'openai' | 'anthropic';
export type AuthType = 'oauth' | 'apikey';
export type AccountStatus = 'active' | 'disabled' | 'error';
export type RegistrationMode = 'open' | 'invite' | 'closed';
export type ReasoningEffort = 'low' | 'medium' | 'high' | 'xhigh' | 'max';
export type RedeemKind = 'balance' | 'plan' | 'invite';
export type RedeemStatus = 'active' | 'revoked' | 'exhausted' | 'expired';
export type UsageEndpoint = 'chat' | 'responses' | 'messages' | 'embeddings';

// ---------- §11 会员梯度 ----------

/** 模型用量池：api = 第三方前沿模型（按 API 价计费），forge = 平台模型。 */
export type ModelPool = 'api' | 'forge';
export type BillingInterval = 'month' | 'year';
/** month = 档位订阅按月重置；period = 额度包整段有效期一个周期。 */
export type UsageCycle = 'month' | 'period';
export type SubscriptionSource = 'grant' | 'redeem' | 'purchase';
export type OrderKind = 'topup' | 'subscription';
export type OrderStatus = 'pending' | 'paid' | 'cancelled' | 'failed';
export type OrderMode = 'new' | 'renew' | 'upgrade' | 'downgrade';

export interface OkResponse {
  ok: boolean;
}

export interface Page<T> {
  items: T[];
  total: number;
}

export interface ItemsResponse<T> {
  items: T[];
}

// ---------- §3.1 认证 ----------

export interface User {
  id: number;
  email: string;
  nickname: string;
  role: Role;
  status: UserStatus;
  hasAvatar: boolean;
  avatarVersion: number;
  groupId: number | null;
  groupName: string;
  balanceMicros: number;
  createdAt: string;
}

export interface DeviceInfo {
  id: string;
  name: string;
  platform: string;
  appVersion: string;
}

export interface LoginRequest {
  email: string;
  password: string;
  device: DeviceInfo;
  issueDeviceKey: boolean;
}

export interface TokenPair {
  accessToken: string;
  accessExpiresAt: string;
  refreshToken: string;
  refreshExpiresAt: string;
}

export interface LoginResponse extends TokenPair {
  user: User;
  deviceKey: { id: number; key: string; prefix: string } | null;
}

export interface AuthConfig {
  registrationMode: RegistrationMode;
  requireEmailVerify: boolean;
  smtpEnabled: boolean;
  siteName: string;
  currency: string;
}

export interface MeResponse {
  user: User;
  subscriptions: Subscription[];
  currency: string;
}

// ---------- §3.2 共享形状 ----------

export interface Subscription {
  id: number;
  planId: number;
  planName: string;
  status: 'active' | 'expired' | 'cancelled';
  startsAt: string;
  endsAt: string;
  /** api 池额度；usedMicros 为当前用量周期（cycleStart–cycleEnd）的已用量。 */
  quotaMicros: number;
  usedMicros: number;
  dailyLimitMicros: number;
  dailyUsedMicros: number;
  groupId: number | null;
  // §11.2 新增
  /** 档位套餐的 tier；额度包为空串。 */
  tier: string;
  forgeQuotaMicros: number;
  forgeUsedMicros: number;
  usageCycle: UsageCycle;
  cycleStart: string;
  cycleEnd: string;
  billingInterval: '' | BillingInterval;
  valueMicros: number;
  source: SubscriptionSource;
}

export interface LedgerEntry {
  id: number;
  deltaMicros: number;
  balanceAfterMicros: number;
  kind: string;
  note: string;
  createdAt: string;
}

export interface Device {
  id: string;
  deviceId: string;
  deviceName: string;
  platform: string;
  appVersion: string;
  ip: string;
  createdAt: string;
  lastSeenAt: string;
  current?: boolean;
  revokedAt?: string | null;
  expiresAt?: string | null;
}

export interface ApiKey {
  id: number;
  name: string;
  kind: 'user' | 'device';
  prefix: string;
  status: 'active' | 'revoked';
  quotaMicros: number;
  usedMicros: number;
  expiresAt: string | null;
  lastUsedAt: string | null;
  createdAt: string;
  deviceName?: string;
}

export interface UsageItem {
  id: number;
  requestId: string;
  model: string;
  endpoint: UsageEndpoint | string;
  stream: boolean;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  costMicros: number;
  status: 'ok' | 'error';
  errorCode: string;
  latencyMs: number;
  createdAt: string;
  apiKeyName: string;
}

export interface UsageSummary {
  requests: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  costMicros: number;
}

// ---------- §7 管理 API ----------

export interface Dashboard {
  users: { total: number; active7d: number };
  accounts: { total: number; active: number; coolingDown: number; error: number };
  today: { requests: number; inputTokens: number; outputTokens: number; costMicros: number; errors: number };
  daily: DashboardDay[];
  topModels: { model: string; requests: number; costMicros: number }[];
  currency: string;
}

export interface DashboardDay {
  date: string;
  requests: number;
  costMicros: number;
  inputTokens: number;
  outputTokens: number;
}

export interface AdminUser extends User {
  lastLoginAt: string | null;
  concurrencyOverride: number | null;
}

export type UserListParams = {
  q?: string;
  status?: UserStatus | '';
  limit?: number;
  offset?: number;
};

export interface CreateUserInput {
  email: string;
  password: string;
  nickname: string;
  role: Role;
  groupId: number | null;
  balanceMicros: number;
}

export interface PatchUserInput {
  nickname?: string;
  role?: Role;
  status?: UserStatus;
  groupId?: number | null;
  concurrencyOverride?: number | null;
}

export interface UserDetail {
  user: AdminUser;
  subscriptions: Subscription[];
  ledger: LedgerEntry[];
  devices: Device[];
  apiKeys: ApiKey[];
}

export interface Group {
  id: number;
  name: string;
  description: string;
  rateMultiplier: number;
  concurrencyLimit: number;
  rpmLimit: number;
  tpmLimit: number;
  allowedModels: string[];
  isDefault: boolean;
  userCount: number;
  accountCount: number;
}

export type GroupInput = Omit<Group, 'id' | 'userCount' | 'accountCount'>;

export interface Plan {
  id: number;
  name: string;
  description: string;
  /** 档位套餐 = 月付价格；额度包 = 价格。 */
  priceMicros: number;
  periodDays: number;
  /** api 池额度（档位：每月；额度包：整个周期）。 */
  quotaMicros: number;
  dailyLimitMicros: number;
  groupId: number | null;
  enabled: boolean;
  // §11.3 新增
  /** 非空 = 会员档位（`^[a-z][a-z0-9_]{0,31}$`，唯一，重复 → 409 TIER_TAKEN）；空串 = 额度包。 */
  tier: string;
  /** 0–1000，越大档位越高。 */
  tierRank: number;
  /** ≤ 64 字。 */
  tagline: string;
  /** ≤ 12 条，每条 ≤ 80 字。 */
  features: string[];
  /** 0 = 不支持按年付费。 */
  priceYearlyMicros: number;
  /** forge 池额度。 */
  forgeQuotaMicros: number;
  highlight: boolean;
  /** 只读：生效订阅数。 */
  subscriberCount: number;
}

export type PlanInput = Omit<Plan, 'id' | 'subscriberCount'>;

/** 额度窗口快照（来自 x-codex-primary-* / x-codex-secondary-* 响应头）。 */
export interface QuotaWindow {
  usedPercent?: number;
  windowMinutes?: number;
  resetAfterSeconds?: number;
  resetsAt?: string;
  resetAt?: string;
  [key: string]: unknown;
}

export interface AccountQuota {
  primary?: QuotaWindow | null;
  secondary?: QuotaWindow | null;
  /** 「刷新额度」时 wham/usage 的原样响应。 */
  usage?: unknown;
  updatedAt?: string;
  [key: string]: unknown;
}

export interface Account {
  id: number;
  name: string;
  platform: Platform;
  authType: AuthType;
  baseUrl: string;
  email: string;
  planType: string;
  status: AccountStatus;
  priority: number;
  weight: number;
  concurrencyLimit: number;
  currentConcurrency: number;
  groupIds: number[];
  modelMapping: Record<string, string>;
  allowedModels?: string[];
  supportsResponses: boolean;
  proxyUrl: string;
  cooldownUntil: string | null;
  lastError: string;
  lastErrorAt: string | null;
  tokenExpiresAt: string | null;
  lastRefreshAt: string | null;
  lastUsedAt: string | null;
  quota: AccountQuota | null;
  keyHint: string;
  createdAt: string;
  updatedAt: string;
}

export type AccountListParams = {
  platform?: Platform | '';
  status?: AccountStatus | '';
  groupId?: number;
  q?: string;
  limit?: number;
  offset?: number;
};

export interface CreateApiKeyAccountInput {
  name: string;
  platform: Platform;
  baseUrl: string;
  apiKey: string;
  supportsResponses: boolean;
  priority: number;
  weight: number;
  concurrencyLimit: number;
  groupIds: number[];
  modelMapping: Record<string, string>;
  allowedModels?: string[];
  proxyUrl: string;
}

export type PatchAccountInput = Partial<CreateApiKeyAccountInput> & { status?: AccountStatus };

export interface ImportCodexItem {
  name?: string;
  authJson: string;
}

export interface ImportCodexInput {
  items: ImportCodexItem[];
  groupIds: number[];
  priority: number;
  concurrencyLimit: number;
  proxyUrl: string;
}

export interface ImportCodexResult {
  created: Account[];
  updated: Account[];
  errors: { index: number; message: string }[];
}

export interface OAuthStartResult {
  sessionId: string;
  authUrl: string;
  redirectUri: string;
  expiresAt: string;
}

export interface OAuthExchangeInput {
  sessionId: string;
  callbackUrl?: string;
  code?: string;
  name?: string;
  groupIds?: number[];
  priority?: number;
  concurrencyLimit?: number;
  proxyUrl?: string;
  accountId?: number;
}

export interface AccountTestResult {
  ok: boolean;
  latencyMs: number;
  httpStatus: number;
  message: string;
}

export interface ModelCapabilities {
  vision: boolean;
  reasoningEfforts: ReasoningEffort[];
  contextWindow: number;
  maxOutput: number;
  tools: boolean;
  responses: boolean;
}

/** 单价：每 1M tokens 的 micros。 */
export interface ModelPricing {
  inputPer1M: number;
  outputPer1M: number;
  cacheReadPer1M: number;
  cacheWritePer1M: number;
}

export interface AdminModel {
  id: string;
  displayName: string;
  platform: Platform;
  upstreamModel: string;
  capabilities: ModelCapabilities;
  pricing: ModelPricing;
  enabled: boolean;
  isDefault: boolean;
  sort: number;
  /** §11.3：用量池，决定扣哪个池的套餐内额度。 */
  pool: ModelPool;
  availableAccounts: number;
}

export type ModelInput = Omit<AdminModel, 'availableAccounts'>;

export interface RedeemCode {
  id: number;
  code: string;
  kind: RedeemKind;
  valueMicros: number;
  planId: number | null;
  planName: string;
  batch: string;
  maxUses: number;
  usedCount: number;
  status: RedeemStatus;
  expiresAt: string | null;
  note: string;
  createdAt: string;
}

export type RedeemListParams = {
  batch?: string;
  status?: RedeemStatus | '';
  kind?: RedeemKind | '';
  q?: string;
  limit?: number;
  offset?: number;
};

export interface CreateRedeemInput {
  kind: RedeemKind;
  valueMicros: number;
  planId: number | null;
  count: number;
  maxUses: number;
  expiresAt: string | null;
  note: string;
  batch: string;
  prefix: string;
}

export interface CreateRedeemResult {
  items: RedeemCode[];
  batch: string;
}

/** §11.2 订单。amountMicros = 应付；listPriceMicros = 档位标价；creditMicros = 升级抵扣。 */
export interface Order {
  id: number;
  kind: OrderKind;
  provider: string;
  status: OrderStatus;
  amountMicros: number;
  listPriceMicros: number;
  creditMicros: number;
  planId: number | null;
  planName: string;
  tier: string;
  /** 充值单为空串。 */
  interval: '' | BillingInterval;
  mode: '' | OrderMode;
  replacesSubscriptionId: number | null;
  subscriptionId: number | null;
  payUrl: string;
  note: string;
  createdAt: string;
  paidAt: string | null;
}

/** §11.3 管理端订单条目。 */
export interface AdminOrder extends Order {
  userId: number;
  userEmail: string;
}

export type OrderListParams = {
  userId?: number;
  status?: OrderStatus | '';
  kind?: OrderKind | '';
  /** 邮箱包含。 */
  q?: string;
  limit?: number;
  offset?: number;
};

export interface AdminUsageItem extends UsageItem {
  userId: number;
  userEmail: string;
  accountId: number | null;
  accountName: string;
  upstreamModel: string;
  httpStatus: number;
  firstTokenMs: number;
  ip: string;
}

export type UsageListParams = {
  userId?: number;
  accountId?: number;
  model?: string;
  status?: 'ok' | 'error' | '';
  from?: string;
  to?: string;
  limit?: number;
  offset?: number;
};

export interface UsagePage extends Page<AdminUsageItem> {
  summary: UsageSummary;
}

export interface Settings {
  siteName: string;
  currency: string;
  registrationMode: RegistrationMode;
  requireEmailVerify: boolean;
  signupBonusMicros: number;
  /** 0 / null = 使用标记为默认的分组。 */
  defaultGroupId: number | null;
  defaultModel: string;
  maxFailoverRetries: number;
  stickyTtlSeconds: number;
  codexInstructions: string;
  /** 只读：是否配置了 SMTP。 */
  smtpEnabled: boolean;
}

export type SettingsInput = Partial<Omit<Settings, 'smtpEnabled'>>;

export interface AuditLog {
  id: number;
  actorId: number | null;
  actorEmail: string;
  action: string;
  target: string;
  detail: Record<string, unknown> | null;
  ip: string;
  createdAt: string;
}

export type AuditListParams = {
  actorId?: number;
  action?: string;
  q?: string;
  limit?: number;
  offset?: number;
};
