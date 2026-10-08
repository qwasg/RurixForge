import { describe, expect, it } from 'vitest';
import {
  dateInputToRfc3339,
  formatAmount,
  formatDuration,
  formatMoney,
  formatNumber,
  formatPercent,
  microsToUnits,
  parseIntInput,
  unitsToMicros,
} from '@/lib/format';

describe('money', () => {
  it('formats micros with 2–6 decimals and the currency code', () => {
    expect(formatAmount(0)).toBe('0.00');
    expect(formatAmount(1_500_000)).toBe('1.50');
    expect(formatAmount(1_234_567_891)).toBe('1,234.567891');
    expect(formatAmount(125_000)).toBe('0.125');
    expect(formatAmount(1)).toBe('0.000001');
    expect(formatAmount(-2_500_000)).toBe('-2.50');
    expect(formatMoney(10_000_000, 'USD')).toBe('10.00 USD');
    expect(formatMoney(10_000_000, 'CNY')).toBe('10.00 CNY');
    expect(formatMoney(null, 'USD')).toBe('—');
  });

  it('converts currency units to integer micros exactly', () => {
    expect(unitsToMicros('1')).toBe(1_000_000);
    expect(unitsToMicros('1.25')).toBe(1_250_000);
    expect(unitsToMicros('0.1')).toBe(100_000);
    expect(unitsToMicros('.5')).toBe(500_000);
    expect(unitsToMicros('-5.5')).toBe(-5_500_000);
    expect(unitsToMicros('1,000.000001')).toBe(1_000_000_001);
    expect(unitsToMicros(' 2 ')).toBe(2_000_000);
    // 第 7 位小数四舍五入
    expect(unitsToMicros('0.0000015')).toBe(2);
    expect(unitsToMicros('0.0000014')).toBe(1);
    expect(unitsToMicros(0.1)).toBe(100_000);
    for (const bad of ['', 'abc', '1.2.3', '1e3', '-']) expect(unitsToMicros(bad)).toBeNaN();
  });

  it('round-trips micros back to form strings', () => {
    expect(microsToUnits(1_250_000)).toBe('1.25');
    expect(microsToUnits(10_000_000)).toBe('10');
    expect(microsToUnits(1)).toBe('0.000001');
    expect(microsToUnits(-500_000)).toBe('-0.5');
    expect(microsToUnits(0)).toBe('0');
    for (const s of ['0.125', '12.5', '1000000', '0.000001']) expect(microsToUnits(unitsToMicros(s))).toBe(s);
  });
});

describe('numbers and time', () => {
  it('adds thousand separators to token counts', () => {
    expect(formatNumber(1234567)).toBe('1,234,567');
    expect(formatNumber(0)).toBe('0');
    expect(formatNumber(undefined)).toBe('—');
  });

  it('formats percents and durations', () => {
    expect(formatPercent(42)).toBe('42%');
    expect(formatPercent(3.25)).toBe('3.3%');
    expect(formatDuration(45)).toBe('45秒');
    expect(formatDuration(200)).toBe('3分20秒');
    expect(formatDuration(3900)).toBe('1小时5分');
    expect(formatDuration(2 * 86400 + 3 * 3600)).toBe('2天3小时');
  });

  it('parses integer inputs', () => {
    expect(parseIntInput('')).toBe(0);
    expect(parseIntInput('', 3)).toBe(3);
    expect(parseIntInput(' 12 ')).toBe(12);
    expect(parseIntInput('1.5')).toBeNaN();
  });

  it('turns local date inputs into RFC3339 day bounds', () => {
    const from = dateInputToRfc3339('2026-09-01');
    const to = dateInputToRfc3339('2026-09-01', true);
    expect(from).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);
    expect(new Date(from!).getTime()).toBe(new Date(2026, 8, 1, 0, 0, 0).getTime());
    expect(new Date(to!).getTime()).toBe(new Date(2026, 8, 1, 23, 59, 59).getTime());
    expect(dateInputToRfc3339('bad')).toBeUndefined();
  });
});
