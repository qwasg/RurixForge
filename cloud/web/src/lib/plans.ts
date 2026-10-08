import type { BillingInterval, Plan, PlanInput, SubscriptionSource } from './api/types';
import { microsToUnits, parseIntInput, unitsToMicros } from './format';
import type { Result } from './models';

/**
 * 套餐 = 会员档位（tier 非空）或额度包（tier 为空），见 15_CLOUD_SERVICE.md §11.1 / §11.3。
 * 档位额度按月重置、可按月/按年购买；额度包整段有效期一个周期。
 */
export type PlanKind = 'tier' | 'pack';

export const INTERVAL_LABEL: Record<BillingInterval, string> = { month: '按月', year: '按年' };
export const SUBSCRIPTION_SOURCE_LABEL: Record<SubscriptionSource, string> = {
  grant: '后台开通',
  redeem: '兑换码',
  purchase: '购买',
};

/** 免费默认档位：其两池额度就是无档位订阅用户每月（UTC 自然月）的免费额度，不可购买。 */
export const FREE_TIER = 'hobby';
export const TIER_RE = /^[a-z][a-z0-9_]{0,31}$/;
export const TIER_RANK_MAX = 1000;
export const TAGLINE_MAX = 64;
export const FEATURES_MAX = 12;
export const FEATURE_MAX = 80;

/** 按字符（码点）计长度，与服务端 rune 计数一致。 */
export function charCount(s: string): number {
  return Array.from(s).length;
}

export function planKind(plan: Pick<Plan, 'tier'>): PlanKind {
  return plan.tier ? 'tier' : 'pack';
}

/** 会员档位按 tierRank 从低到高（同级按 ID）；额度包保持接口顺序。 */
export function splitPlans(plans: Plan[]): { tiers: Plan[]; packs: Plan[] } {
  const tiers = plans.filter((p) => !!p.tier).sort((a, b) => (a.tierRank ?? 0) - (b.tierRank ?? 0) || a.id - b.id);
  const packs = plans.filter((p) => !p.tier);
  return { tiers, packs };
}

/** 「权益」文本框：一行一条，去掉首尾空白与空行。 */
export function parseFeatures(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== '');
}

export interface YearlyDiscount {
  /** 12 × 月付。 */
  fullMicros: number;
  /** 年付折合每月（四舍五入到 micros）。 */
  perMonthMicros: number;
  /** 比 12 × 月付省下的金额（年付更贵时为负）。 */
  savedMicros: number;
  /** 年付 / (12 × 月付) 的百分比，向下取整（80 = 8 折）。 */
  percent: number;
}

/** 年付相对 12 × 月付的折扣；任一价格不为正时返回 null。 */
export function yearlyDiscount(monthlyMicros: number, yearlyMicros: number): YearlyDiscount | null {
  if (!(monthlyMicros > 0) || !(yearlyMicros > 0)) return null;
  const fullMicros = monthlyMicros * 12;
  return {
    fullMicros,
    perMonthMicros: Math.round(yearlyMicros / 12),
    savedMicros: fullMicros - yearlyMicros,
    percent: Math.floor((yearlyMicros * 100) / fullMicros),
  };
}

/** 80 → 「8 折」，83 → 「8.3 折」；≥ 100 视为没有折扣，返回空串。 */
export function formatZhe(percent: number): string {
  if (!(percent < 100)) return '';
  const p = Math.max(1, percent);
  return `${p % 10 === 0 ? p / 10 : (p / 10).toFixed(1)} 折`;
}

/** 表单状态：金额为额度单位的十进制字符串，整数为字符串，features 为一行一条的文本。 */
export interface PlanForm {
  kind: PlanKind;
  name: string;
  description: string;
  tier: string;
  tierRank: string;
  tagline: string;
  features: string;
  /** 档位 = 月付价格；额度包 = 价格。 */
  price: string;
  priceYearly: string;
  periodDays: string;
  /** api 池额度。 */
  quota: string;
  /** forge 池额度。 */
  forgeQuota: string;
  dailyLimit: string;
  groupId: string;
  enabled: boolean;
  highlight: boolean;
}

export function planToForm(plan: Plan | null, kind: PlanKind = plan ? planKind(plan) : 'pack'): PlanForm {
  return {
    kind,
    name: plan?.name ?? '',
    description: plan?.description ?? '',
    tier: plan?.tier ?? '',
    tierRank: String(plan?.tierRank ?? 0),
    tagline: plan?.tagline ?? '',
    features: (plan?.features ?? []).join('\n'),
    price: plan ? microsToUnits(plan.priceMicros) : '',
    priceYearly: plan && plan.priceYearlyMicros > 0 ? microsToUnits(plan.priceYearlyMicros) : '',
    periodDays: String(plan?.periodDays ?? 30),
    quota: plan ? microsToUnits(plan.quotaMicros) : '',
    forgeQuota: plan ? microsToUnits(plan.forgeQuotaMicros ?? 0) : '',
    dailyLimit: plan ? microsToUnits(plan.dailyLimitMicros) : '0',
    groupId: plan?.groupId == null ? '' : String(plan.groupId),
    enabled: plan?.enabled ?? true,
    highlight: plan?.highlight ?? false,
  };
}

/** 空串 = 0；负数或非法数字返回错误文案。 */
function moneyField(label: string, value: string): number | string {
  const n = value.trim() === '' ? 0 : unitsToMicros(value);
  if (Number.isNaN(n) || n < 0) return `${label}应为非负数字`;
  return n;
}

/**
 * 表单 → PlanInput。额度包只提交通用字段，档位专属字段（tier、tierRank、tagline、features、
 * 年付价格、推荐）一律归零，避免把隐藏的旧值带给额度包。
 */
export function planFromForm(form: PlanForm): Result<PlanInput> {
  const name = form.name.trim();
  if (!name) return { ok: false, error: '请填写套餐名称' };
  const isTier = form.kind === 'tier';
  const priceMicros = moneyField(isTier ? '月付价格' : '价格', form.price);
  const quotaMicros = moneyField('第三方 API 池额度', form.quota);
  const forgeQuotaMicros = moneyField('平台模型池额度', form.forgeQuota);
  const dailyLimitMicros = moneyField('每日上限', form.dailyLimit);
  for (const v of [priceMicros, quotaMicros, forgeQuotaMicros, dailyLimitMicros]) if (typeof v === 'string') return { ok: false, error: v };
  const periodDays = parseIntInput(form.periodDays);
  if (Number.isNaN(periodDays) || periodDays <= 0) return { ok: false, error: isTier ? '开通天数应为正整数' : '周期天数应为正整数' };

  const common = {
    name,
    description: form.description.trim(),
    priceMicros: priceMicros as number,
    periodDays,
    quotaMicros: quotaMicros as number,
    forgeQuotaMicros: forgeQuotaMicros as number,
    dailyLimitMicros: dailyLimitMicros as number,
    groupId: form.groupId ? Number(form.groupId) : null,
    enabled: form.enabled,
  };
  if (!isTier) {
    return {
      ok: true,
      value: { ...common, tier: '', tierRank: 0, tagline: '', features: [], priceYearlyMicros: 0, highlight: false },
    };
  }

  const tier = form.tier.trim();
  if (!tier) return { ok: false, error: '请填写档位标识' };
  if (!TIER_RE.test(tier)) return { ok: false, error: '档位标识须以小写字母开头，只含小写字母、数字和下划线，最长 32 位' };
  const tierRank = parseIntInput(form.tierRank);
  if (Number.isNaN(tierRank) || tierRank < 0 || tierRank > TIER_RANK_MAX) return { ok: false, error: `档位级别应为 0–${TIER_RANK_MAX} 的整数` };
  const tagline = form.tagline.trim();
  if (charCount(tagline) > TAGLINE_MAX) return { ok: false, error: `标语最多 ${TAGLINE_MAX} 字` };
  const features = parseFeatures(form.features);
  if (features.length > FEATURES_MAX) return { ok: false, error: `权益最多 ${FEATURES_MAX} 条（当前 ${features.length} 条）` };
  const tooLong = features.findIndex((f) => charCount(f) > FEATURE_MAX);
  if (tooLong >= 0) return { ok: false, error: `第 ${tooLong + 1} 条权益超过 ${FEATURE_MAX} 字` };
  const priceYearlyMicros = moneyField('年付价格', form.priceYearly);
  if (typeof priceYearlyMicros === 'string') return { ok: false, error: priceYearlyMicros };

  return {
    ok: true,
    value: { ...common, tier, tierRank, tagline, features, priceYearlyMicros, highlight: form.highlight },
  };
}
