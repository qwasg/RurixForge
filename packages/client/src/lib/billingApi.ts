/**
 * 会员梯度与额度计费(D-042,15_CLOUD_SERVICE.md §11):客户端经 host → agentd 的
 * `/api/forge/account/*` 透传访问 forge-cloud 的档位 / 会员 / 报价 / 购买 / 订单接口。
 * 金额一律整数 micros(1 额度单位 = 1_000_000 micros),展示复用 accountApi 的 formatMoney。
 */

import {
  ACCOUNT_BASE,
  AccountApiError,
  accountQuery,
  accountRequest,
  type Subscription,
} from './accountApi';

// ---------- 形状(§11.2) ----------

export type BillingInterval = 'month' | 'year';
export type PoolId = 'api' | 'forge';
export type QuoteMode = 'new' | 'renew' | 'upgrade' | 'downgrade';

export interface PaymentInfo {
  enabled: boolean;
  /** 在线渠道名(余额支付 `balance` 恒可用,不在此列)。 */
  providers: string[];
}

export interface Tier {
  planId: number;
  tier: string;
  name: string;
  tagline: string;
  description: string;
  features: string[];
  priceMonthlyMicros: number;
  priceYearlyMicros: number;
  includedApiMicros: number;
  includedForgeMicros: number;
  dailyLimitMicros: number;
  highlight: boolean;
  rank: number;
}

export interface TiersResponse {
  currency: string;
  payment: PaymentInfo;
  items: Tier[];
}

/** §3.2 Subscription + §11.2 新增字段(usedMicros / forgeUsedMicros 为当前用量周期的已用量)。 */
export interface MembershipSubscription extends Subscription {
  tier?: string;
  forgeQuotaMicros?: number;
  forgeUsedMicros?: number;
  usageCycle?: 'month' | 'period' | string;
  cycleStart?: string;
  cycleEnd?: string;
  billingInterval?: '' | BillingInterval | string;
  valueMicros?: number;
  source?: 'grant' | 'redeem' | 'purchase' | string;
}

export interface Pool {
  includedMicros: number;
  usedMicros: number;
  remainingMicros: number;
}

export interface OnDemand {
  enabled: boolean;
  /** 0 = 不设上限。 */
  limitMicros: number;
  usedMicros: number;
}

export interface Order {
  id: number;
  kind: 'topup' | 'subscription' | string;
  provider: string;
  status: 'pending' | 'paid' | 'cancelled' | 'failed' | string;
  amountMicros: number;
  listPriceMicros: number;
  creditMicros: number;
  planId: number | null;
  planName: string;
  tier: string;
  interval: string;
  mode: string;
  replacesSubscriptionId: number | null;
  subscriptionId: number | null;
  payUrl: string;
  note: string;
  createdAt: string;
  paidAt: string | null;
}

export interface Membership {
  currency: string;
  tier: { planId: number; tier: string; name: string; rank: number };
  subscription: MembershipSubscription | null;
  scheduled: MembershipSubscription[];
  packs: MembershipSubscription[];
  cycle: { start: string; end: string };
  pools: Record<PoolId, Pool>;
  onDemand: OnDemand;
  balanceMicros: number;
  payment: PaymentInfo;
  pendingOrder: Order | null;
}

export interface ModelUsage {
  model: string;
  pool: PoolId | string;
  requests: number;
  errors: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  costMicros: number;
  includedMicros: number;
  onDemandMicros: number;
}

export interface MembershipUsage {
  from: string;
  to: string;
  currency: string;
  items: ModelUsage[];
  totals: ModelUsage;
}

export interface Quote {
  planId: number;
  tier: string;
  planName: string;
  interval: BillingInterval | string;
  mode: QuoteMode | string;
  listPriceMicros: number;
  creditMicros: number;
  amountMicros: number;
  refundMicros: number;
  startsAt: string;
  endsAt: string;
  currentSubscriptionId: number | null;
  replacesSubscriptionIds: number[];
  balanceMicros: number;
  currency: string;
  payment: PaymentInfo;
}

export interface CheckoutResult {
  order: Order;
  membership: Membership;
}

export interface OrdersPage {
  items: Order[];
  total: number;
}

// ---------- 归一(缺字段不让页面崩) ----------

function num(v: unknown, fallback = 0): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback;
}

function str(v: unknown, fallback = ''): string {
  return typeof v === 'string' ? v : fallback;
}

function obj(v: unknown): Record<string, unknown> {
  return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
}

function normalizePayment(raw: unknown): PaymentInfo {
  const p = obj(raw);
  const providers = Array.isArray(p.providers)
    ? p.providers.filter((x): x is string => typeof x === 'string' && x !== '' && x !== 'balance')
    : [];
  return { enabled: p.enabled === true && providers.length > 0, providers };
}

function normalizePool(raw: unknown): Pool {
  const p = obj(raw);
  const includedMicros = num(p.includedMicros);
  const usedMicros = num(p.usedMicros);
  return {
    includedMicros,
    usedMicros,
    remainingMicros: num(p.remainingMicros, Math.max(includedMicros - usedMicros, 0)),
  };
}

function subs(v: unknown): MembershipSubscription[] {
  return Array.isArray(v) ? (v as MembershipSubscription[]) : [];
}

export function normalizeMembership(raw: unknown): Membership {
  const m = obj(raw);
  const tier = obj(m.tier);
  const cycle = obj(m.cycle);
  const pools = obj(m.pools);
  const od = obj(m.onDemand);
  return {
    currency: str(m.currency, 'USD') || 'USD',
    tier: { planId: num(tier.planId), tier: str(tier.tier, 'hobby'), name: str(tier.name, 'Hobby'), rank: num(tier.rank) },
    subscription: m.subscription && typeof m.subscription === 'object' ? (m.subscription as MembershipSubscription) : null,
    scheduled: subs(m.scheduled),
    packs: subs(m.packs),
    cycle: { start: str(cycle.start), end: str(cycle.end) },
    pools: { api: normalizePool(pools.api), forge: normalizePool(pools.forge) },
    onDemand: { enabled: od.enabled !== false, limitMicros: num(od.limitMicros), usedMicros: num(od.usedMicros) },
    balanceMicros: num(m.balanceMicros),
    payment: normalizePayment(m.payment),
    pendingOrder: m.pendingOrder && typeof m.pendingOrder === 'object' ? (m.pendingOrder as Order) : null,
  };
}

export function normalizeTiers(raw: unknown): TiersResponse {
  const r = obj(raw);
  const items = (Array.isArray(r.items) ? r.items : []).map((x) => {
    const t = obj(x);
    return {
      planId: num(t.planId),
      tier: str(t.tier),
      name: str(t.name),
      tagline: str(t.tagline),
      description: str(t.description),
      features: Array.isArray(t.features) ? t.features.filter((f): f is string => typeof f === 'string') : [],
      priceMonthlyMicros: num(t.priceMonthlyMicros),
      priceYearlyMicros: num(t.priceYearlyMicros),
      includedApiMicros: num(t.includedApiMicros),
      includedForgeMicros: num(t.includedForgeMicros),
      dailyLimitMicros: num(t.dailyLimitMicros),
      highlight: t.highlight === true,
      rank: num(t.rank),
    };
  });
  items.sort((a, b) => a.rank - b.rank);
  return { currency: str(r.currency, 'USD') || 'USD', payment: normalizePayment(r.payment), items };
}

function normalizeModelUsage(raw: unknown): ModelUsage {
  const u = obj(raw);
  return {
    model: str(u.model),
    pool: str(u.pool),
    requests: num(u.requests),
    errors: num(u.errors),
    inputTokens: num(u.inputTokens),
    outputTokens: num(u.outputTokens),
    cacheReadTokens: num(u.cacheReadTokens),
    cacheWriteTokens: num(u.cacheWriteTokens),
    costMicros: num(u.costMicros),
    includedMicros: num(u.includedMicros),
    onDemandMicros: num(u.onDemandMicros),
  };
}

export function normalizeUsage(raw: unknown): MembershipUsage {
  const r = obj(raw);
  return {
    from: str(r.from),
    to: str(r.to),
    currency: str(r.currency, 'USD') || 'USD',
    items: (Array.isArray(r.items) ? r.items : []).map(normalizeModelUsage),
    totals: normalizeModelUsage(r.totals),
  };
}

function normalizeOrders(raw: unknown): OrdersPage {
  const r = obj(raw);
  return {
    items: Array.isArray(r.items) ? (r.items as Order[]) : [],
    total: num(r.total),
  };
}

// ---------- /api/forge/account/*(§11.4) ----------

export async function getTiers(): Promise<TiersResponse> {
  return normalizeTiers(await accountRequest<unknown>('GET', `${ACCOUNT_BASE}/tiers`));
}

export async function getMembership(): Promise<Membership> {
  return normalizeMembership(await accountRequest<unknown>('GET', `${ACCOUNT_BASE}/membership`));
}

/** limitMicros 0–1e12(0 = 不限)。 */
export function patchOnDemand(patch: { enabled?: boolean; limitMicros?: number }): Promise<OnDemand> {
  return accountRequest('PATCH', `${ACCOUNT_BASE}/membership/on-demand`, patch);
}

/** 缺省 = 当前用量周期,按费用倒序。 */
export async function getMembershipUsage(params: { from?: string; to?: string } = {}): Promise<MembershipUsage> {
  return normalizeUsage(await accountRequest<unknown>('GET', `${ACCOUNT_BASE}/membership/usage${accountQuery(params)}`));
}

/** 只读报价。 */
export function postQuote(payload: { planId: number; interval: BillingInterval }): Promise<Quote> {
  return accountRequest('POST', `${ACCOUNT_BASE}/membership/quote`, payload);
}

/** provider `balance` = 余额支付(余额须 ≥ 应付,否则 402 INSUFFICIENT_BALANCE)。 */
export async function postCheckout(payload: {
  planId: number;
  interval: BillingInterval;
  provider: string;
}): Promise<CheckoutResult> {
  const r = await accountRequest<{ order: Order; membership: unknown }>('POST', `${ACCOUNT_BASE}/membership/checkout`, payload);
  return { order: r.order, membership: normalizeMembership(r.membership) };
}

/** 取消排在最后、尚未开始的预约;已购价值全额退回余额。 */
export async function cancelScheduled(id: number): Promise<Membership> {
  return normalizeMembership(
    await accountRequest<unknown>('DELETE', `${ACCOUNT_BASE}/membership/scheduled/${encodeURIComponent(String(id))}`),
  );
}

export async function listOrders(params: { limit?: number; offset?: number } = {}): Promise<OrdersPage> {
  return normalizeOrders(await accountRequest<unknown>('GET', `${ACCOUNT_BASE}/orders${accountQuery(params)}`));
}

export function cancelOrder(id: number): Promise<Order> {
  return accountRequest('POST', `${ACCOUNT_BASE}/orders/${encodeURIComponent(String(id))}/cancel`, {});
}

// ---------- 展示与规则(纯函数) ----------

export const POOL_LABELS: Record<PoolId, string> = {
  api: '第三方模型（按 API 价）',
  forge: '平台模型',
};

export function poolLabel(pool: string): string {
  return pool === 'api' || pool === 'forge' ? POOL_LABELS[pool] : pool;
}

export function intervalLabel(interval: string | undefined | null): string {
  if (interval === 'month') return '按月';
  if (interval === 'year') return '按年';
  return '';
}

const MODE_LABELS: Record<QuoteMode, string> = {
  new: '开通',
  renew: '续订',
  upgrade: '升级',
  downgrade: '降级',
};

export function modeLabel(mode: string | undefined | null): string {
  return mode && mode in MODE_LABELS ? MODE_LABELS[mode as QuoteMode] : (mode ?? '');
}

const ORDER_STATUS: Record<string, string> = {
  pending: '待支付',
  paid: '已支付',
  cancelled: '已取消',
  failed: '失败',
};

export function orderStatusLabel(status: string): string {
  return ORDER_STATUS[status] ?? status;
}

export function providerLabel(provider: string): string {
  return provider === 'balance' || provider === '' ? '余额' : provider;
}

/** 「Pro+ · 按年 · 升级」;充值单 →「充值」。 */
export function orderSummary(o: Pick<Order, 'kind' | 'planName' | 'interval' | 'mode'>): string {
  if (o.kind !== 'subscription') return o.kind === 'topup' ? '充值' : o.kind;
  return [o.planName || '会员', intervalLabel(o.interval), modeLabel(o.mode)].filter((s) => s !== '').join(' · ');
}

/** 在线渠道的支付页地址:只放行 http/https(拒绝 javascript: 等),否则 null。 */
export function safePayUrl(url: string | null | undefined): string | null {
  const u = typeof url === 'string' ? url.trim() : '';
  return /^https?:\/\/[^\s]+$/i.test(u) ? u : null;
}

/** 免费档(Hobby):月付年付都是 0,不可购买。 */
export function isFreeTier(t: Pick<Tier, 'priceMonthlyMicros' | 'priceYearlyMicros'>): boolean {
  return t.priceMonthlyMicros <= 0 && t.priceYearlyMicros <= 0;
}

/** 该档位能否按此周期购买(免费档不可;标价 0 的周期视为未开放)。 */
export function tierPurchasable(t: Tier, interval: BillingInterval): boolean {
  if (isFreeTier(t)) return false;
  return (interval === 'year' ? t.priceYearlyMicros : t.priceMonthlyMicros) > 0;
}

/** 年付相对「月付 × 12」省下的百分比(整数,四舍五入;无年价或不省 → 0)。 */
export function savingsPercent(monthlyMicros: number, yearlyMicros: number): number {
  if (monthlyMicros <= 0 || yearlyMicros <= 0) return 0;
  const pct = Math.round((1 - yearlyMicros / (monthlyMicros * 12)) * 100);
  return pct > 0 ? pct : 0;
}

/** 年价折合每月(micros,取整)。 */
export function perMonthMicros(yearlyMicros: number): number {
  return Math.round(yearlyMicros / 12);
}

export type TierActionKind = 'free' | 'new' | 'upgrade' | 'downgrade' | 'renew';

export interface TierAction {
  kind: TierActionKind;
  /** 按钮文案;免费档为 null(不给按钮)。 */
  label: string | null;
  /** 是否是用户的当前有效档位。 */
  current: boolean;
}

const ACTION_LABELS: Record<Exclude<TierActionKind, 'free'>, string> = {
  new: '开通',
  upgrade: '升级',
  downgrade: '降级',
  renew: '续订',
};

/**
 * 档位按钮(§11.1 报价规则):没有生效中的档位订阅 → 开通;按 rank 与当前档位比:更高 → 升级、
 * 更低 → 降级、同档 → 续订(含改付费周期)。免费档恒不给按钮。
 */
export function tierAction(t: Tier, m: Pick<Membership, 'tier' | 'subscription'>): TierAction {
  const current = t.tier !== '' && t.tier === m.tier.tier;
  if (isFreeTier(t)) return { kind: 'free', label: null, current };
  let kind: TierActionKind;
  if (!m.subscription) kind = 'new';
  else if (t.rank > m.tier.rank) kind = 'upgrade';
  else if (t.rank < m.tier.rank) kind = 'downgrade';
  else kind = 'renew';
  return { kind, label: ACTION_LABELS[kind], current };
}

/** 金额输入(额度单位,最多 6 位小数)→ micros;非法、负数或超过 1e12 micros → null。 */
export function parseAmountToMicros(input: string): number | null {
  const s = input.trim();
  const m = /^(\d{1,7})(?:\.(\d{0,6}))?$/.exec(s);
  if (!m) return null;
  const micros = Number(m[1]) * 1_000_000 + Number((m[2] ?? '').padEnd(6, '0'));
  return micros <= 1_000_000_000_000 ? micros : null;
}

/** micros → 输入框用的纯数字(5_500_000 → 「5.5」,无千分位)。 */
export function microsToPlain(micros: number): string {
  const v = Math.max(0, Math.round(micros));
  const whole = Math.floor(v / 1_000_000);
  const frac = String(v % 1_000_000).padStart(6, '0').replace(/0+$/, '');
  return frac === '' ? String(whole) : `${whole}.${frac}`;
}

export interface Shortfall {
  balanceMicros: number;
  amountMicros: number;
  shortMicros: number;
}

/** 402 INSUFFICIENT_BALANCE 的差额(错误体顶层 balanceMicros / amountMicros);其它错误 → null。 */
export function shortfallFromError(err: unknown): Shortfall | null {
  if (!(err instanceof AccountApiError) || err.code !== 'INSUFFICIENT_BALANCE') return null;
  const b = obj(err.body);
  if (typeof b.balanceMicros !== 'number' || typeof b.amountMicros !== 'number') return null;
  return {
    balanceMicros: b.balanceMicros,
    amountMicros: b.amountMicros,
    shortMicros: Math.max(b.amountMicros - b.balanceMicros, 0),
  };
}
