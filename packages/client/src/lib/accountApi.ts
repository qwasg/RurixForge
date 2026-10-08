/**
 * 云账户面(D-041 / 15_CLOUD_SERVICE.md §8.2–§8.4):客户端只经 host → agentd(BFF)访问
 * `/api/forge/account/*` 与 `/api/forge/memory`,永远拿不到令牌;唯一明文是用户自建 API Key
 * 的创建响应(只显示一次)。金额一律整数 micros(1 额度单位 = 1_000_000 micros)。
 */

import { ForgeApiError } from './forgeApi';

// ---------- 形状(§3.1 / §3.2 / §3.3 / §8) ----------

export interface CloudUser {
  id: number;
  email: string;
  nickname: string;
  role: 'user' | 'admin' | string;
  status: 'active' | 'disabled' | string;
  hasAvatar: boolean;
  avatarVersion: number;
  groupId?: number | null;
  groupName?: string;
  balanceMicros: number;
  createdAt?: string;
}

export interface Subscription {
  id: number;
  planId: number;
  planName: string;
  status: 'active' | 'expired' | 'cancelled' | string;
  startsAt: string;
  endsAt: string;
  quotaMicros: number;
  usedMicros: number;
  dailyLimitMicros: number;
  dailyUsedMicros: number;
  groupId?: number | null;
}

export interface SyncFlags {
  settings: boolean;
  memory: boolean;
  skills: boolean;
}

export interface AccountSyncState extends SyncFlags {
  lastSyncAt: string | null;
  lastError: string | null;
}

/** GET /api/forge/account/status。 */
export interface AccountStatus {
  serverUrl: string;
  loggedIn: boolean;
  reachable: boolean;
  user: CloudUser | null;
  balanceMicros: number;
  currency: string;
  subscriptions: Subscription[];
  deviceKeyPrefix: string | null;
  byoAllowed: boolean;
  byoConfigured: boolean;
  devMock: boolean;
  sync: AccountSyncState;
  lastError: string | null;
}

/** GET|POST /api/forge/account/config(data/cloud-config.json 的非密视图)。 */
export interface CloudConfig {
  serverUrl: string;
  deviceId?: string;
  deviceName?: string;
  sync?: Partial<SyncFlags>;
}

export type RegistrationMode = 'open' | 'invite' | 'closed';

/** GET /api/forge/account/auth-config(透传云端 /api/v1/auth/config)。 */
export interface AuthConfig {
  registrationMode: RegistrationMode;
  requireEmailVerify: boolean;
  smtpEnabled: boolean;
  siteName: string;
  currency: string;
}

export interface RegisterPayload {
  email: string;
  password: string;
  nickname?: string;
  inviteCode?: string;
  emailCode?: string;
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
  current: boolean;
}

export interface ApiKey {
  id: number;
  name: string;
  kind: 'user' | 'device' | string;
  prefix: string;
  status: 'active' | 'revoked' | string;
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
  endpoint: string;
  stream: boolean;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  costMicros: number;
  status: 'ok' | 'error' | string;
  errorCode?: string | null;
  latencyMs: number;
  createdAt: string;
  apiKeyName?: string | null;
}

export interface UsageSummary {
  requests: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  costMicros: number;
}

export interface UsagePage {
  items: UsageItem[];
  total: number;
  summary?: UsageSummary;
}

export interface UsageDay {
  date: string;
  requests: number;
  inputTokens: number;
  outputTokens: number;
  costMicros: number;
}

export interface LedgerEntry {
  id: number;
  deltaMicros: number;
  balanceAfterMicros: number;
  kind: string;
  note: string;
  createdAt: string;
}

export interface RedeemResult {
  kind: 'balance' | 'plan' | string;
  valueMicros: number;
  plan: { id: number; name: string } | null;
  subscription: Subscription | null;
  balanceMicros: number;
}

export interface Plan {
  id: number;
  name: string;
  description: string;
  priceMicros: number;
  periodDays: number;
  quotaMicros: number;
  dailyLimitMicros: number;
}

export interface CloudModelCapabilities {
  vision: boolean;
  reasoningEfforts: string[];
  thinkingMode?: 'manual' | 'adaptive';
  thinkingAlwaysOn?: boolean;
  contextWindow: number;
  maxOutput: number;
  tools: boolean;
  responses: boolean;
}

/** 单价:micros / 1M tokens(已乘分组倍率)。 */
export interface CloudModelPricing {
  inputPer1M: number;
  outputPer1M: number;
  cacheReadPer1M: number;
  cacheWritePer1M: number;
}

export interface CloudModel {
  id: string;
  displayName: string;
  platform: string;
  capabilities: CloudModelCapabilities;
  pricing: CloudModelPricing;
  available: boolean;
}

/** GET /api/forge/account/models(云端模型目录,agentd 缓存 5 分钟)。 */
export interface CloudCatalog {
  defaultModel: string;
  currency: string;
  rateMultiplier: number;
  models: CloudModel[];
}

export interface CloudSettingEntry {
  value: unknown;
  version: number;
  updatedAt: string;
}

export interface CloudSettingsResponse {
  items: Record<string, CloudSettingEntry>;
  source?: 'cloud' | 'cache';
}

export interface CloudSettingPutResult {
  namespace: string;
  value: unknown;
  version: number;
  updatedAt: string;
  pending?: boolean;
}

// ---------- 记忆(§8.3) ----------

export type MemoryKind = 'preference' | 'fact' | 'convention';
export type MemoryScopeFilter = 'all' | 'global' | 'project';

export interface LocalMemory {
  id: string;
  /** `global` 或 `project:<key>`。 */
  scope: string;
  kind: MemoryKind | string;
  content: string;
  tags: string[];
  createdAt: string;
  updatedAt: string;
  source?: 'agent' | 'user' | string;
}

export interface MemoryListResponse {
  items: LocalMemory[];
  projectKey?: string;
  sync?: { enabled: boolean; lastSyncAt: string | null };
}

// ---------- 传输 ----------

const ACCOUNT = '/api/forge/account';
const MEMORY = '/api/forge/memory';

/**
 * 账户面 API 错误:在 ForgeApiError 之上保留云端原始错误体——§11.2 的 402 `INSUFFICIENT_BALANCE`
 * 在顶层附 `balanceMicros` / `amountMicros`,界面据此显示差额。
 */
export class AccountApiError extends ForgeApiError {
  body: unknown;
  constructor(code: string, message: string, status: number, body: unknown) {
    super(code, message, status);
    this.body = body;
  }
}

async function readJson(res: Response): Promise<unknown> {
  try {
    return await res.json();
  } catch {
    return undefined;
  }
}

/** 统一请求:网络错 → NETWORK;非 2xx → 取 `{error:{code,message}}`(message 直接给 UI 用,原始体留在 `body`)。 */
async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  let res: Response;
  try {
    res = await fetch(
      path,
      body === undefined
        ? { method }
        : { method, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) },
    );
  } catch (err) {
    throw new ForgeApiError('NETWORK', `无法连接本机服务:${(err as Error).message}`);
  }
  const data = await readJson(res);
  if (!res.ok) {
    const e = (data as { error?: { code?: string; message?: string } } | undefined)?.error;
    throw new AccountApiError(e?.code ?? `HTTP_${res.status}`, e?.message ?? `HTTP ${res.status}`, res.status, data);
  }
  return data as T;
}

function query(params: Record<string, string | number | null | undefined>): string {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === null || v === '') continue;
    q.set(k, String(v));
  }
  const s = q.toString();
  return s === '' ? '' : `?${s}`;
}

/** 同一传输给会员面(billingApi.ts)复用。 */
export { request as accountRequest, query as accountQuery, ACCOUNT as ACCOUNT_BASE };

// ---------- 状态归一 ----------

function num(v: unknown, fallback = 0): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback;
}

function str(v: unknown, fallback = ''): string {
  return typeof v === 'string' ? v : fallback;
}

export function normalizeUser(raw: unknown): CloudUser | null {
  if (!raw || typeof raw !== 'object') return null;
  const u = raw as Record<string, unknown>;
  return {
    id: num(u.id),
    email: str(u.email),
    nickname: str(u.nickname),
    role: str(u.role, 'user'),
    status: str(u.status, 'active'),
    hasAvatar: u.hasAvatar === true,
    avatarVersion: num(u.avatarVersion),
    groupId: typeof u.groupId === 'number' ? u.groupId : null,
    groupName: str(u.groupName),
    balanceMicros: num(u.balanceMicros),
    createdAt: str(u.createdAt),
  };
}

/**
 * 只有 agentd 明确回了布尔 `loggedIn` 才算拿到状态;旧后端 / 桩返回的其它形状一律视为未知(null),
 * 未知态不弹登录门。
 */
export function normalizeStatus(raw: unknown): AccountStatus | null {
  if (!raw || typeof raw !== 'object') return null;
  const s = raw as Record<string, unknown>;
  if (typeof s.loggedIn !== 'boolean') return null;
  const sync = (s.sync && typeof s.sync === 'object' ? s.sync : {}) as Record<string, unknown>;
  const user = normalizeUser(s.user);
  return {
    serverUrl: str(s.serverUrl),
    loggedIn: s.loggedIn,
    reachable: s.reachable !== false,
    user,
    balanceMicros: num(s.balanceMicros, user?.balanceMicros ?? 0),
    currency: str(s.currency, 'USD') || 'USD',
    subscriptions: Array.isArray(s.subscriptions) ? (s.subscriptions as Subscription[]) : [],
    deviceKeyPrefix: typeof s.deviceKeyPrefix === 'string' ? s.deviceKeyPrefix : null,
    byoAllowed: s.byoAllowed !== false,
    byoConfigured: s.byoConfigured === true,
    devMock: s.devMock === true,
    sync: {
      settings: sync.settings !== false,
      memory: sync.memory !== false,
      skills: sync.skills !== false,
      lastSyncAt: typeof sync.lastSyncAt === 'string' ? sync.lastSyncAt : null,
      lastError: typeof sync.lastError === 'string' ? sync.lastError : null,
    },
    lastError: typeof s.lastError === 'string' && s.lastError !== '' ? s.lastError : null,
  };
}

export function normalizeAuthConfig(raw: unknown): AuthConfig | null {
  if (!raw || typeof raw !== 'object') return null;
  const c = raw as Record<string, unknown>;
  const mode = c.registrationMode;
  return {
    registrationMode: mode === 'invite' || mode === 'closed' ? mode : 'open',
    requireEmailVerify: c.requireEmailVerify === true,
    smtpEnabled: c.smtpEnabled === true,
    siteName: str(c.siteName, 'RurixForge Cloud') || 'RurixForge Cloud',
    currency: str(c.currency, 'USD') || 'USD',
  };
}

// ---------- /api/forge/account/*(§8.2) ----------

export async function getAccountStatus(): Promise<AccountStatus | null> {
  return normalizeStatus(await request<unknown>('GET', `${ACCOUNT}/status`));
}

export function getAccountConfig(): Promise<CloudConfig> {
  return request('GET', `${ACCOUNT}/config`);
}

/** 已登录时改 serverUrl → 409 LOGGED_IN。 */
export function postAccountConfig(patch: {
  serverUrl?: string;
  deviceName?: string;
  sync?: Partial<SyncFlags>;
}): Promise<CloudConfig> {
  return request('POST', `${ACCOUNT}/config`, patch);
}

export async function getAuthConfig(): Promise<AuthConfig | null> {
  return normalizeAuthConfig(await request<unknown>('GET', `${ACCOUNT}/auth-config`));
}

export async function postLogin(payload: { email: string; password: string }): Promise<AccountStatus | null> {
  return normalizeStatus(await request<unknown>('POST', `${ACCOUNT}/login`, payload));
}

export async function postRegister(payload: RegisterPayload): Promise<AccountStatus | null> {
  return normalizeStatus(await request<unknown>('POST', `${ACCOUNT}/register`, payload));
}

export function postEmailCode(payload: { email: string; purpose: 'register' | 'reset' }): Promise<{ ok: boolean }> {
  return request('POST', `${ACCOUNT}/email-code`, payload);
}

/** 云端不可达也会清本地令牌。 */
export async function postLogout(): Promise<AccountStatus | null> {
  return normalizeStatus(await request<unknown>('POST', `${ACCOUNT}/logout`, {}));
}

export function getMe(): Promise<{ user: CloudUser; subscriptions: Subscription[]; currency: string }> {
  return request('GET', `${ACCOUNT}/me`);
}

export function patchProfile(payload: { nickname: string }): Promise<CloudUser> {
  return request('PATCH', `${ACCOUNT}/profile`, payload);
}

/** 成功后云端吊销其它会话。 */
export function postPassword(payload: { oldPassword: string; newPassword: string }): Promise<{ ok: boolean }> {
  return request('POST', `${ACCOUNT}/password`, payload);
}

/** 头像图片地址;version 变化即换 URL,绕开缓存。 */
export function avatarUrl(version: number): string {
  return `${ACCOUNT}/avatar?v=${encodeURIComponent(String(version))}`;
}

/** dataUrl 仅 png/jpeg/webp/gif,≤512 KiB(400 AVATAR_INVALID / 413 AVATAR_TOO_LARGE)。 */
export function putAvatar(dataUrl: string): Promise<CloudUser> {
  return request('PUT', `${ACCOUNT}/avatar`, { dataUrl });
}

export function deleteAvatar(): Promise<CloudUser> {
  return request('DELETE', `${ACCOUNT}/avatar`);
}

export function listDevices(): Promise<{ items: Device[] }> {
  return request('GET', `${ACCOUNT}/devices`);
}

/** 删的是本机会话 → agentd 同时本地登出。 */
export function deleteDevice(id: string): Promise<{ ok: boolean }> {
  return request('DELETE', `${ACCOUNT}/devices/${encodeURIComponent(id)}`);
}

export function listApiKeys(): Promise<{ items: ApiKey[] }> {
  return request('GET', `${ACCOUNT}/api-keys`);
}

/** `key` 明文只在本响应里出现一次。 */
export function createApiKey(payload: {
  name: string;
  quotaMicros?: number;
  expiresAt?: string | null;
}): Promise<{ apiKey: ApiKey; key: string }> {
  return request('POST', `${ACCOUNT}/api-keys`, payload);
}

export function deleteApiKey(id: number): Promise<{ ok: boolean }> {
  return request('DELETE', `${ACCOUNT}/api-keys/${encodeURIComponent(String(id))}`);
}

export function getBalance(): Promise<{ balanceMicros: number; currency: string; subscriptions: Subscription[] }> {
  return request('GET', `${ACCOUNT}/balance`);
}

export function getSubscriptions(): Promise<{ items: Subscription[] }> {
  return request('GET', `${ACCOUNT}/subscription`);
}

export function getUsage(params: {
  from?: string;
  to?: string;
  limit?: number;
  offset?: number;
} = {}): Promise<UsagePage> {
  return request('GET', `${ACCOUNT}/usage${query(params)}`);
}

export function getUsageDaily(days = 30): Promise<{ items: UsageDay[] }> {
  return request('GET', `${ACCOUNT}/usage/daily${query({ days })}`);
}

export function getLedger(params: { limit?: number; offset?: number } = {}): Promise<{
  items: LedgerEntry[];
  total: number;
}> {
  return request('GET', `${ACCOUNT}/ledger${query(params)}`);
}

export function postRedeem(code: string): Promise<RedeemResult> {
  return request('POST', `${ACCOUNT}/redeem`, { code });
}

export function getPlans(): Promise<{ items: Plan[] }> {
  return request('GET', `${ACCOUNT}/plans`);
}

export function getCloudModels(): Promise<CloudCatalog> {
  return request('GET', `${ACCOUNT}/models`);
}

export function getCloudSettings(): Promise<CloudSettingsResponse> {
  return request('GET', `${ACCOUNT}/settings`);
}

/** 离线时 agentd 写本地缓存并回 `pending:true`,恢复后补推。 */
export function putCloudSetting(ns: string, value: unknown): Promise<CloudSettingPutResult> {
  return request('PUT', `${ACCOUNT}/settings/${encodeURIComponent(ns)}`, { value });
}

export function postSync(): Promise<{ ok: boolean; sync: AccountSyncState }> {
  return request('POST', `${ACCOUNT}/sync`, {});
}

// ---------- /api/forge/memory(§8.3) ----------

export function listMemories(params: { q?: string; scope?: MemoryScopeFilter } = {}): Promise<MemoryListResponse> {
  return request('GET', `${MEMORY}${query({ q: params.q?.trim(), scope: params.scope })}`);
}

export function createMemory(payload: {
  content: string;
  kind: MemoryKind;
  scope: 'global' | 'project';
  tags: string[];
}): Promise<LocalMemory> {
  return request('POST', MEMORY, payload);
}

export function patchMemory(
  id: string,
  payload: { content?: string; kind?: MemoryKind; tags?: string[] },
): Promise<LocalMemory> {
  return request('PATCH', `${MEMORY}/${encodeURIComponent(id)}`, payload);
}

export function deleteMemory(id: string): Promise<{ ok: boolean }> {
  return request('DELETE', `${MEMORY}/${encodeURIComponent(id)}`);
}

// ---------- 技能(§8.4:新建带 scope) ----------

export type SkillScope = 'personal' | 'workspace';

/** scope 缺省由 agentd 取 personal(data/user-skills/<name>/,随账号同步)。 */
export function createSkill(name: string, scope?: SkillScope): Promise<unknown> {
  return request('POST', '/api/forge/skills', scope ? { name, scope } : { name });
}

// ---------- 展示 ----------

/** 外部 Codex CLI 走本平台网关的 config.toml 片段(密钥经环境变量 FORGE_CLOUD_API_KEY 注入)。 */
export function codexCliSnippet(serverUrl: string): string {
  const base = (serverUrl.trim() || 'http://127.0.0.1:8110').replace(/\/+$/, '');
  return [
    'model_provider = "forge-cloud"',
    '',
    '[model_providers.forge-cloud]',
    'name = "RurixForge Cloud"',
    `base_url = "${base}/v1"`,
    'env_key = "FORGE_CLOUD_API_KEY"',
    'wire_api = "responses"',
  ].join('\n');
}

/** micros → 「1.25」:整数运算,小数 2–6 位(去掉 2 位之后的尾零)。 */
export function formatAmount(micros: number | null | undefined): string {
  const v = typeof micros === 'number' && Number.isFinite(micros) ? Math.round(micros) : 0;
  const abs = Math.abs(v);
  const whole = Math.floor(abs / 1_000_000);
  let frac = String(abs % 1_000_000).padStart(6, '0').replace(/0+$/, '');
  if (frac.length < 2) frac = frac.padEnd(2, '0');
  const sign = v < 0 ? '-' : '';
  return `${sign}${whole.toLocaleString('en-US')}.${frac}`;
}

/** micros → 「1.25 USD」。 */
export function formatMoney(micros: number | null | undefined, currency = 'USD'): string {
  return `${formatAmount(micros)} ${currency || 'USD'}`;
}

/** token 数 → 400K / 1.05M(上下文窗口等展示用)。 */
export function formatTokens(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0';
  const trim = (x: number) => String(Math.round(x * 100) / 100);
  if (n >= 1_000_000) return `${trim(n / 1_000_000)}M`;
  if (n >= 1_000) return `${trim(n / 1_000)}K`;
  return String(Math.round(n));
}

/** RFC3339 → 本地短时间(非法值原样返回)。 */
export function formatTime(ts: string | null | undefined): string {
  if (!ts) return '—';
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return d.toLocaleString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/** 错误 → 给用户看的文案(优先 API 的 error.message)。 */
export function errorMessage(err: unknown): string {
  if (err instanceof ForgeApiError) return err.message;
  return err instanceof Error ? err.message : String(err);
}
