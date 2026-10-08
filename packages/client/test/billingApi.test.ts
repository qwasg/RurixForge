import { describe, expect, it } from 'vitest';
import { quoteNote } from '@/components/settings/billing/CheckoutDialog';
import { tierPrice } from '@/components/settings/billing/TiersSection';
import { AccountApiError } from '@/lib/accountApi';
import {
  microsToPlain,
  normalizeMembership,
  normalizeTiers,
  orderSummary,
  parseAmountToMicros,
  perMonthMicros,
  safePayUrl,
  savingsPercent,
  shortfallFromError,
  tierAction,
  tierPurchasable,
  type MembershipSubscription,
  type Tier,
} from '@/lib/billingApi';

/** 会员梯度与额度计费(D-042,15 §11)的客户端纯函数。 */

function tier(over: Partial<Tier> = {}): Tier {
  return {
    planId: 2,
    tier: 'pro',
    name: 'Pro',
    tagline: '',
    description: '',
    features: [],
    priceMonthlyMicros: 20_000_000,
    priceYearlyMicros: 192_000_000,
    includedApiMicros: 20_000_000,
    includedForgeMicros: 60_000_000,
    dailyLimitMicros: 0,
    highlight: false,
    rank: 10,
    ...over,
  };
}

const hobby = tier({
  planId: 1,
  tier: 'hobby',
  name: 'Hobby',
  priceMonthlyMicros: 0,
  priceYearlyMicros: 0,
  includedApiMicros: 0,
  includedForgeMicros: 1_000_000,
  rank: 0,
});
const pro = tier();
const proPlus = tier({ planId: 3, tier: 'pro_plus', name: 'Pro+', priceMonthlyMicros: 60_000_000, priceYearlyMicros: 576_000_000, rank: 20 });

const proSub: MembershipSubscription = {
  id: 11,
  planId: 2,
  planName: 'Pro',
  status: 'active',
  startsAt: '2026-09-10T00:00:00Z',
  endsAt: '2026-10-10T00:00:00Z',
  quotaMicros: 20_000_000,
  usedMicros: 0,
  dailyLimitMicros: 0,
  dailyUsedMicros: 0,
};

describe('tierAction(§11.1 报价规则对应的按钮)', () => {
  it('没有档位订阅:付费档都是「开通」,Hobby 是当前档位且不给按钮', () => {
    const m = { tier: { planId: 1, tier: 'hobby', name: 'Hobby', rank: 0 }, subscription: null };
    expect(tierAction(hobby, m)).toEqual({ kind: 'free', label: null, current: true });
    expect(tierAction(pro, m)).toEqual({ kind: 'new', label: '开通', current: false });
    expect(tierAction(proPlus, m).kind).toBe('new');
  });

  it('有 Pro 订阅:同档续订、更高升级、更低降级,免费档仍无按钮', () => {
    const m = { tier: { planId: 2, tier: 'pro', name: 'Pro', rank: 10 }, subscription: proSub };
    expect(tierAction(pro, m)).toEqual({ kind: 'renew', label: '续订', current: true });
    expect(tierAction(proPlus, m)).toEqual({ kind: 'upgrade', label: '升级', current: false });
    expect(tierAction(tier({ tier: 'lite', rank: 5 }), m)).toEqual({ kind: 'downgrade', label: '降级', current: false });
    expect(tierAction(hobby, m)).toEqual({ kind: 'free', label: null, current: false });
  });
});

describe('价格', () => {
  it('年付折扣、折合月价与可购买性', () => {
    expect(savingsPercent(20_000_000, 192_000_000)).toBe(20);
    expect(savingsPercent(20_000_000, 240_000_000)).toBe(0);
    expect(savingsPercent(0, 192_000_000)).toBe(0);
    expect(perMonthMicros(192_000_000)).toBe(16_000_000);
    expect(tierPurchasable(pro, 'year')).toBe(true);
    expect(tierPurchasable(tier({ priceYearlyMicros: 0 }), 'year')).toBe(false);
    expect(tierPurchasable(hobby, 'month')).toBe(false);
  });

  it('档位卡上的价格文案', () => {
    expect(tierPrice(pro, 'month', 'USD')).toEqual({ main: '20.00 USD', unit: '/ 月', note: '按月付费,可随时升级' });
    expect(tierPrice(pro, 'year', 'USD')).toEqual({ main: '16.00 USD', unit: '/ 月', note: '按年付 192.00 USD,省 20%' });
    expect(tierPrice(hobby, 'year', 'USD').main).toBe('免费');
    expect(tierPrice(tier({ priceYearlyMicros: 0 }), 'year', 'USD').note).toBe('暂不支持按年购买');
  });
});

describe('金额输入', () => {
  it('额度单位 ↔ micros,拒绝负数、超 6 位小数与超过 1e12 micros', () => {
    expect(parseAmountToMicros('25.5')).toBe(25_500_000);
    expect(parseAmountToMicros(' 0 ')).toBe(0);
    expect(parseAmountToMicros('1000000')).toBe(1_000_000_000_000);
    expect(parseAmountToMicros('1000001')).toBeNull();
    expect(parseAmountToMicros('1.1234567')).toBeNull();
    expect(parseAmountToMicros('-1')).toBeNull();
    expect(parseAmountToMicros('abc')).toBeNull();
    expect(parseAmountToMicros('')).toBeNull();
    expect(microsToPlain(5_500_000)).toBe('5.5');
    expect(microsToPlain(20_000_000)).toBe('20');
    expect(microsToPlain(1)).toBe('0.000001');
  });
});

describe('错误与归一', () => {
  it('402 INSUFFICIENT_BALANCE 的差额只取错误体顶层金额', () => {
    const body = {
      error: { code: 'INSUFFICIENT_BALANCE', message: '余额不足' },
      balanceMicros: 30_000_000,
      amountMicros: 48_000_000,
    };
    expect(shortfallFromError(new AccountApiError('INSUFFICIENT_BALANCE', '余额不足', 402, body))).toEqual({
      balanceMicros: 30_000_000,
      amountMicros: 48_000_000,
      shortMicros: 18_000_000,
    });
    expect(shortfallFromError(new AccountApiError('INSUFFICIENT_BALANCE', '余额不足', 402, { error: {} }))).toBeNull();
    expect(shortfallFromError(new AccountApiError('PAYMENT_FAILED', '支付失败', 502, body))).toBeNull();
    expect(shortfallFromError(new Error('boom'))).toBeNull();
  });

  it('normalizeMembership 缺字段兜底为 Hobby;normalizeTiers 按 rank 排序且不把 balance 当在线渠道', () => {
    const m = normalizeMembership({});
    expect(m.tier).toEqual({ planId: 0, tier: 'hobby', name: 'Hobby', rank: 0 });
    expect(m.onDemand).toEqual({ enabled: true, limitMicros: 0, usedMicros: 0 });
    expect(m.pools.forge).toEqual({ includedMicros: 0, usedMicros: 0, remainingMicros: 0 });
    expect(m.scheduled).toEqual([]);
    expect(m.payment).toEqual({ enabled: false, providers: [] });

    const t = normalizeTiers({
      currency: 'CNY',
      payment: { enabled: true, providers: ['balance', 'alipay'] },
      items: [
        { tier: 'ultra', rank: 30 },
        { tier: 'pro', rank: 10, features: ['a', 1] },
      ],
    });
    expect(t.items.map((x) => x.tier)).toEqual(['pro', 'ultra']);
    expect(t.items[0].features).toEqual(['a']);
    expect(t.payment).toEqual({ enabled: true, providers: ['alipay'] });
    expect(t.currency).toBe('CNY');
  });
});

describe('订单与报价文案', () => {
  it('订单摘要;支付地址只放行 http(s)', () => {
    expect(orderSummary({ kind: 'subscription', planName: 'Pro+', interval: 'year', mode: 'upgrade' })).toBe('Pro+ · 按年 · 升级');
    expect(orderSummary({ kind: 'topup', planName: '', interval: '', mode: '' })).toBe('充值');
    expect(safePayUrl(' https://pay.example.test/o/1 ')).toBe('https://pay.example.test/o/1');
    expect(safePayUrl('javascript:alert(1)')).toBeNull();
    expect(safePayUrl('')).toBeNull();
    expect(safePayUrl(null)).toBeNull();
  });

  it('报价说明按模式区分', () => {
    const at = { startsAt: '2026-10-10T00:00:00Z', endsAt: '2026-11-10T00:00:00Z' };
    expect(quoteNote({ mode: 'upgrade', planName: 'Pro+', ...at })).toContain('折算抵扣');
    expect(quoteNote({ mode: 'downgrade', planName: 'Pro', ...at })).toContain('切换为「Pro」');
    expect(quoteNote({ mode: 'renew', planName: 'Pro', ...at })).toContain('接在当前订阅之后');
    expect(quoteNote({ mode: 'new', planName: 'Pro', ...at })).toContain('立即生效');
  });
});
