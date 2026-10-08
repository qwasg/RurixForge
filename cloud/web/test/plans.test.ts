import { describe, expect, it } from 'vitest';
import type { Plan } from '@/lib/api/types';
import { formatZhe, parseFeatures, planFromForm, planToForm, splitPlans, yearlyDiscount } from '@/lib/plans';

function plan(over: Partial<Plan>): Plan {
  return {
    id: 1,
    name: 'P',
    description: '',
    priceMicros: 0,
    periodDays: 30,
    quotaMicros: 0,
    dailyLimitMicros: 0,
    groupId: null,
    enabled: true,
    tier: '',
    tierRank: 0,
    tagline: '',
    features: [],
    priceYearlyMicros: 0,
    forgeQuotaMicros: 0,
    highlight: false,
    subscriberCount: 0,
    ...over,
  };
}

describe('planFromForm', () => {
  it('serializes tier fields, trimming features and dropping blank lines', () => {
    const form = {
      ...planToForm(null, 'tier'),
      name: ' Team ',
      tier: 'team',
      tierRank: '40',
      tagline: '团队协作',
      features: ' 统一账单 \n\nSSO\r\n',
      price: '40',
      priceYearly: '384',
      quota: '50',
      forgeQuota: '150',
      highlight: true,
    };
    expect(planFromForm(form)).toEqual({
      ok: true,
      value: {
        name: 'Team',
        description: '',
        priceMicros: 40_000_000,
        periodDays: 30,
        quotaMicros: 50_000_000,
        forgeQuotaMicros: 150_000_000,
        dailyLimitMicros: 0,
        groupId: null,
        enabled: true,
        tier: 'team',
        tierRank: 40,
        tagline: '团队协作',
        features: ['统一账单', 'SSO'],
        priceYearlyMicros: 384_000_000,
        highlight: true,
      },
    });
  });

  it('resets tier-only fields when saving a pack', () => {
    const existing = plan({ tier: 'pro', tierRank: 10, tagline: 'x', features: ['a'], priceYearlyMicros: 5, highlight: true });
    const r = planFromForm({ ...planToForm(existing, 'pack'), price: '9.9', quota: '10' });
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.value).toMatchObject({ tier: '', tierRank: 0, tagline: '', features: [], priceYearlyMicros: 0, highlight: false });
    expect(r.value.priceMicros).toBe(9_900_000);
  });

  it('rejects invalid tier ids, ranks and too many features', () => {
    const base = { ...planToForm(null, 'tier'), name: 'X', tier: 'ok', price: '1' };
    expect(planFromForm({ ...base, tier: '' })).toMatchObject({ ok: false });
    expect(planFromForm({ ...base, tier: '9lives' })).toMatchObject({ ok: false });
    expect(planFromForm({ ...base, tier: 'Pro' })).toMatchObject({ ok: false });
    expect(planFromForm({ ...base, tierRank: '1001' })).toMatchObject({ ok: false });
    const many = Array.from({ length: 13 }, (_, i) => `权益 ${i}`).join('\n');
    expect(planFromForm({ ...base, features: many })).toMatchObject({ ok: false });
    expect(planFromForm({ ...base, priceYearly: '-1' })).toMatchObject({ ok: false });
    expect(planFromForm({ ...base, name: '  ' })).toMatchObject({ ok: false });
  });
});

describe('yearly pricing helpers', () => {
  it('computes the Cursor-style 20% yearly discount', () => {
    expect(yearlyDiscount(20_000_000, 192_000_000)).toEqual({
      fullMicros: 240_000_000,
      perMonthMicros: 16_000_000,
      savedMicros: 48_000_000,
      percent: 80,
    });
    expect(yearlyDiscount(0, 192_000_000)).toBeNull();
    expect(yearlyDiscount(20_000_000, 0)).toBeNull();
  });

  it('formats 折 labels', () => {
    expect(formatZhe(80)).toBe('8 折');
    expect(formatZhe(83)).toBe('8.3 折');
    expect(formatZhe(100)).toBe('');
    expect(formatZhe(120)).toBe('');
  });
});

describe('splitPlans / parseFeatures', () => {
  it('sorts tiers by rank and keeps packs separate', () => {
    const { tiers, packs } = splitPlans([
      plan({ id: 3, tier: 'ultra', tierRank: 30 }),
      plan({ id: 9, name: 'Pack' }),
      plan({ id: 1, tier: 'hobby', tierRank: 0 }),
      plan({ id: 2, tier: 'pro', tierRank: 10 }),
    ]);
    expect(tiers.map((p) => p.tier)).toEqual(['hobby', 'pro', 'ultra']);
    expect(packs.map((p) => p.id)).toEqual([9]);
  });

  it('parses one feature per line', () => {
    expect(parseFeatures('a\r\n\n  b  \n')).toEqual(['a', 'b']);
  });
});
