/** 金额一律整数 micros：1 额度单位 = 1_000_000 micros（15_CLOUD_SERVICE.md §1）。 */
export const MICROS_PER_UNIT = 1_000_000;

const EMPTY = '—';

const amountFormatter = new Intl.NumberFormat('en-US', {
  minimumFractionDigits: 2,
  maximumFractionDigits: 6,
});
const integerFormatter = new Intl.NumberFormat('en-US');
const compactFormatter = new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 });
const dateTimeFormatter = new Intl.DateTimeFormat('zh-CN', {
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
});
const dateFormatter = new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit' });

function isNum(n: number | null | undefined): n is number {
  return typeof n === 'number' && Number.isFinite(n);
}

/** micros → 「1,234.50」（2–6 位小数）。 */
export function formatAmount(micros: number | null | undefined): string {
  if (!isNum(micros)) return EMPTY;
  const units = micros / MICROS_PER_UNIT;
  return amountFormatter.format(Object.is(units, -0) ? 0 : units);
}

/** micros → 「1,234.50 USD」。currency 是系统设置里的展示货币（可能不是 ISO 代码，原样拼接）。 */
export function formatMoney(micros: number | null | undefined, currency = 'USD'): string {
  const amount = formatAmount(micros);
  if (amount === EMPTY || !currency) return amount;
  return `${amount} ${currency}`;
}

/** token 数等整数：千分位。 */
export function formatNumber(n: number | null | undefined): string {
  return isNum(n) ? integerFormatter.format(n) : EMPTY;
}

export const formatTokens = formatNumber;

/** 图表刻度用的紧凑写法：1.2K / 3.4M。 */
export function formatCompact(n: number | null | undefined): string {
  return isNum(n) ? compactFormatter.format(n) : EMPTY;
}

export function formatPercent(p: number | null | undefined): string {
  if (!isNum(p)) return EMPTY;
  return `${p < 10 && p % 1 !== 0 ? p.toFixed(1) : Math.round(p)}%`;
}

function parseTime(value: string | null | undefined): Date | null {
  if (!value) return null;
  const d = new Date(value);
  // Go 零值时间（0001-01-01）视作空。
  if (Number.isNaN(d.getTime()) || d.getUTCFullYear() <= 1) return null;
  return d;
}

/** RFC3339 → 本地时间「2026/09/26 10:42:00」。 */
export function formatDateTime(value: string | null | undefined): string {
  const d = parseTime(value);
  return d ? dateTimeFormatter.format(d) : EMPTY;
}

export function formatDate(value: string | null | undefined): string {
  const d = parseTime(value);
  return d ? dateFormatter.format(d) : EMPTY;
}

/** 距 value 还有多少秒（已过去为负；无效为 null）。 */
export function secondsUntil(value: string | null | undefined, now: number = Date.now()): number | null {
  const d = parseTime(value);
  return d ? (d.getTime() - now) / 1000 : null;
}

/** 秒数 → 「2天3小时」「1小时5分」「3分20秒」「45秒」。 */
export function formatDuration(totalSeconds: number): string {
  const s = Math.max(0, Math.round(totalSeconds));
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (d > 0) return h > 0 ? `${d}天${h}小时` : `${d}天`;
  if (h > 0) return m > 0 ? `${h}小时${m}分` : `${h}小时`;
  if (m > 0) return sec > 0 ? `${m}分${sec}秒` : `${m}分`;
  return `${sec}秒`;
}

/** 相对现在：「3天后」「5分钟前」。 */
export function formatRelative(value: string | null | undefined, now: number = Date.now()): string {
  const secs = secondsUntil(value, now);
  if (secs === null) return EMPTY;
  const abs = Math.abs(secs);
  let text: string;
  if (abs < 60) text = `${Math.round(abs)}秒`;
  else if (abs < 3600) text = `${Math.round(abs / 60)}分钟`;
  else if (abs < 86400) text = `${Math.round(abs / 3600)}小时`;
  else text = `${Math.round(abs / 86400)}天`;
  return secs >= 0 ? `${text}后` : `${text}前`;
}

/** 分钟窗口 → 「5小时」「7天」。 */
export function formatWindowMinutes(minutes: number | null | undefined): string {
  if (!isNum(minutes) || minutes <= 0) return '';
  if (minutes % 1440 === 0) return `${minutes / 1440}天`;
  if (minutes % 60 === 0) return `${minutes / 60}小时`;
  return `${minutes}分钟`;
}

/**
 * 额度单位（十进制字符串或数字）→ micros（整数）。
 * 字符串按十进制精确换算，超过 6 位小数时第 7 位四舍五入；非法输入返回 NaN。
 */
export function unitsToMicros(input: string | number): number {
  if (typeof input === 'number') {
    return Number.isFinite(input) ? Math.round(input * MICROS_PER_UNIT) : Number.NaN;
  }
  const m = /^([+-])?(\d*)(?:\.(\d*))?$/.exec(input.trim().replace(/,/g, ''));
  if (!m) return Number.NaN;
  const intPart = m[2] ?? '';
  const fracPart = m[3] ?? '';
  if (intPart === '' && fracPart === '') return Number.NaN;
  let micros = Number(intPart || '0') * MICROS_PER_UNIT + Number(fracPart.padEnd(6, '0').slice(0, 6));
  if (fracPart.length > 6 && Number(fracPart[6]) >= 5) micros += 1;
  if (!Number.isSafeInteger(micros)) return Number.NaN;
  return m[1] === '-' && micros !== 0 ? -micros : micros;
}

/** micros → 表单里的额度字符串（无千分位、去掉末尾 0）：1250000 → "1.25"。 */
export function microsToUnits(micros: number | null | undefined): string {
  if (!isNum(micros)) return '';
  const sign = micros < 0 ? '-' : '';
  const abs = Math.abs(Math.round(micros));
  const intPart = Math.floor(abs / MICROS_PER_UNIT);
  const frac = String(abs % MICROS_PER_UNIT).padStart(6, '0').replace(/0+$/, '');
  return `${sign}${intPart}${frac ? `.${frac}` : ''}`;
}

/** Date → RFC3339（UTC，去掉毫秒）。 */
export function toRfc3339(date: Date): string {
  return date.toISOString().replace(/\.\d{3}Z$/, 'Z');
}

/** <input type="date"> 的 YYYY-MM-DD（本地日期）→ 当天 00:00:00 或 23:59:59 的 RFC3339。 */
export function dateInputToRfc3339(value: string, endOfDay = false): string | undefined {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!m) return undefined;
  const d = endOfDay
    ? new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]), 23, 59, 59)
    : new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]), 0, 0, 0);
  return toRfc3339(d);
}

/** <input type="datetime-local">（本地时间）→ RFC3339。 */
export function dateTimeLocalToRfc3339(value: string): string | undefined {
  if (!value) return undefined;
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? undefined : toRfc3339(d);
}

/** 解析非负整数输入；空串给 fallback，非法返回 NaN。 */
export function parseIntInput(value: string, fallback = 0): number {
  const s = value.trim();
  if (s === '') return fallback;
  return /^-?\d+$/.test(s) ? Number(s) : Number.NaN;
}
