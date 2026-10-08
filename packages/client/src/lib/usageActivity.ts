import type { UsageDay } from './accountApi';

const DAY_MS = 86_400_000;
export const ACTIVITY_DAYS = 365;
export type ActivityMode = 'daily' | 'weekly' | 'total';

export function fillDays(items: UsageDay[], n = 30, today = new Date()): UsageDay[] {
  const byDate = new Map(items.map((item) => [item.date, item]));
  const end = Date.parse(`${today.toISOString().slice(0, 10)}T00:00:00Z`);
  return Array.from({ length: n }, (_, index) => {
    const date = new Date(end - (n - 1 - index) * DAY_MS).toISOString().slice(0, 10);
    return byDate.get(date) ?? { date, requests: 0, inputTokens: 0, outputTokens: 0, costMicros: 0 };
  });
}

export function recentUsageRange(today = new Date()) {
  const end = Date.parse(`${today.toISOString().slice(0, 10)}T00:00:00Z`);
  return { from: new Date(end - 29 * DAY_MS).toISOString(), to: new Date(end + DAY_MS).toISOString() };
}

export interface ActivityCell {
  date: string;
  endDate: string;
  week: number;
  row: number;
  value: number;
  day: UsageDay;
}

/** Calendar weeks start on Sunday; only dates actually requested are included. */
export function buildUsageActivity(items: UsageDay[], mode: ActivityMode, today = new Date()) {
  const days = fillDays(items, ACTIVITY_DAYS, today);
  const firstRow = new Date(`${days[0].date}T00:00:00Z`).getUTCDay();
  const columns = Math.ceil((firstRow + days.length) / 7);
  const tokens = (day: UsageDay) => [day.inputTokens, day.outputTokens]
    .reduce((sum, value) => sum + (Number.isFinite(value) ? Math.max(0, value) : 0), 0);
  let total = 0;
  const daily: ActivityCell[] = days.map((day, index) => {
    const value = tokens(day);
    total += value;
    return { date: day.date, endDate: day.date, week: Math.floor((firstRow + index) / 7),
      row: (firstRow + index) % 7, value: mode === 'total' ? total : value, day };
  });
  const cells = mode === 'weekly' ? Array.from({ length: columns }, (_, week): ActivityCell => {
    const members = daily.filter((cell) => cell.week === week);
    return { ...members[0], endDate: members[members.length - 1].date, row: 0,
      value: members.reduce((sum, cell) => sum + cell.value, 0) };
  }) : daily;
  const months = daily.filter((cell) => cell.date.endsWith('-01'))
    .map((cell) => ({ date: cell.date, week: cell.week, label: `${Number(cell.date.slice(5, 7))}月` }));
  return { cells, months, columns, total, firstDate: days[0].date, lastDate: days[days.length - 1].date };
}

export function activityLevel(value: number, max: number): number {
  return value > 0 && max > 0 ? Math.min(4, Math.max(1, Math.ceil(value / max * 4))) : 0;
}
