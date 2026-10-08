import type { Account, AccountQuota, AccountStatus, Platform, QuotaWindow } from './api/types';
import { formatWindowMinutes, parseIntInput } from './format';
import type { Result } from './models';

export const PLATFORM_LABEL: Record<Platform, string> = { openai: 'OpenAI', anthropic: 'Anthropic' };

export const ACCOUNT_STATUS_LABEL: Record<AccountStatus, string> = {
  active: '正常',
  disabled: '已停用',
  error: '异常',
};

export interface BaseUrlPreset {
  id: string;
  label: string;
  platform: Platform;
  baseUrl: string;
  supportsResponses: boolean;
}

/** apikey 账号的 baseUrl 含版本段（§4.1），网关只拼 /chat/completions 等后缀。 */
export const BASE_URL_PRESETS: BaseUrlPreset[] = [
  { id: 'openai', label: 'OpenAI', platform: 'openai', baseUrl: 'https://api.openai.com/v1', supportsResponses: true },
  { id: 'deepseek', label: 'DeepSeek', platform: 'openai', baseUrl: 'https://api.deepseek.com/v1', supportsResponses: false },
  {
    id: 'qwen',
    label: '通义千问',
    platform: 'openai',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    supportsResponses: false,
  },
  { id: 'kimi', label: 'Kimi', platform: 'openai', baseUrl: 'https://api.moonshot.cn/v1', supportsResponses: false },
  { id: 'glm', label: '智谱 GLM', platform: 'openai', baseUrl: 'https://open.bigmodel.cn/api/paas/v4', supportsResponses: false },
  { id: 'openrouter', label: 'OpenRouter', platform: 'openai', baseUrl: 'https://openrouter.ai/api/v1', supportsResponses: false },
  {
    id: 'anthropic',
    label: 'Anthropic',
    platform: 'anthropic',
    baseUrl: 'https://api.anthropic.com/v1',
    supportsResponses: false,
  },
];

export const CUSTOM_PRESET = 'custom';

export function presetForUrl(url: string): string {
  const u = url.trim().replace(/\/+$/, '');
  return BASE_URL_PRESETS.find((p) => p.baseUrl === u)?.id ?? CUSTOM_PRESET;
}

export const COMPLIANCE_HINT = '订阅账号对外商用可能违反上游服务条款，建议对外流量以 API Key 账号为主';

export interface QuotaBarInfo {
  key: 'primary' | 'secondary';
  label: string;
  percent: number;
  /** 重置时刻（ms），未知为 null。 */
  resetAt: number | null;
}

function num(v: unknown): number | undefined {
  if (typeof v === 'number' && Number.isFinite(v)) return v;
  if (typeof v === 'string' && v.trim() !== '' && Number.isFinite(Number(v))) return Number(v);
  return undefined;
}

function timeValue(v: unknown): number | null {
  if (typeof v === 'string' && v) {
    const t = Date.parse(v);
    if (!Number.isNaN(t)) return t;
    const n = num(v);
    if (n !== undefined) return n < 1e12 ? n * 1000 : n;
  }
  if (typeof v === 'number' && Number.isFinite(v)) return v < 1e12 ? v * 1000 : v;
  return null;
}

/** quota.primary / quota.secondary → 额度条（兼容 camelCase 与 snake_case 键）。 */
export function quotaBars(quota: AccountQuota | null | undefined): QuotaBarInfo[] {
  if (!quota) return [];
  const updatedAt = timeValue(quota.updatedAt);
  const out: QuotaBarInfo[] = [];
  for (const key of ['primary', 'secondary'] as const) {
    const w = quota[key] as QuotaWindow | null | undefined;
    if (!w || typeof w !== 'object') continue;
    const percent = num(w.usedPercent) ?? num(w.used_percent);
    if (percent === undefined) continue;
    const minutes = num(w.windowMinutes) ?? num(w.window_minutes);
    let resetAt = timeValue(w.resetsAt) ?? timeValue(w.resetAt) ?? timeValue(w.resets_at) ?? timeValue(w.reset_at);
    const after = num(w.resetAfterSeconds) ?? num(w.reset_after_seconds);
    if (resetAt === null && after !== undefined && updatedAt !== null) resetAt = updatedAt + after * 1000;
    out.push({
      key,
      label: formatWindowMinutes(minutes) || (key === 'primary' ? '主' : '次'),
      percent: Math.max(0, Math.min(100, percent)),
      resetAt,
    });
  }
  return out;
}

/** 冷却剩余秒数；未冷却返回 0。 */
export function cooldownRemaining(account: Pick<Account, 'cooldownUntil'>, now: number): number {
  if (!account.cooldownUntil) return 0;
  const t = Date.parse(account.cooldownUntil);
  if (Number.isNaN(t)) return 0;
  return Math.max(0, Math.round((t - now) / 1000));
}

export function isAccount(v: unknown): v is Account {
  return !!v && typeof v === 'object' && typeof (v as Account).id === 'number' && typeof (v as Account).platform === 'string';
}

/**
 * OAuth 回调：粘贴完整回调 URL（推荐）或只粘贴 code。
 * 回调页 http://localhost:1455/auth/callback 本身打不开，URL 里的 code/state 才是要的。
 */
export function parseOAuthCallback(input: string): { callbackUrl: string } | { code: string } | { error: string } {
  const s = input.trim();
  if (!s) return { error: '请粘贴浏览器地址栏中的回调 URL' };
  if (/^https?:\/\//i.test(s)) {
    let url: URL;
    try {
      url = new URL(s);
    } catch {
      return { error: '回调 URL 格式不正确' };
    }
    const err = url.searchParams.get('error');
    if (err) return { error: `授权失败：${url.searchParams.get('error_description') || err}` };
    if (!url.searchParams.get('code')) return { error: '回调 URL 中没有 code 参数，请复制跳转后的完整地址' };
    return { callbackUrl: s };
  }
  if (/\s/.test(s) || s.includes('?')) return { error: '请粘贴完整的回调 URL' };
  return { code: s };
}

/** 账号池参数（添加/编辑账号共用）的表单值。 */
export interface PoolForm {
  groupIds: number[];
  priority: string;
  weight: string;
  concurrencyLimit: string;
  proxyUrl: string;
}

export interface PoolValues {
  groupIds: number[];
  priority: number;
  weight: number;
  concurrencyLimit: number;
  proxyUrl: string;
}

export function isValidProxyUrl(v: string): boolean {
  const s = v.trim();
  return s === '' || /^(https?|socks5h?):\/\/\S+$/i.test(s);
}

export function parsePool(form: PoolForm): Result<PoolValues> {
  const priority = parseIntInput(form.priority);
  const weight = parseIntInput(form.weight, 1);
  const concurrencyLimit = parseIntInput(form.concurrencyLimit);
  if (Number.isNaN(priority) || priority < 0) return { ok: false, error: '优先级应为非负整数（数值越小越优先）' };
  if (Number.isNaN(weight) || weight < 0) return { ok: false, error: '权重应为非负整数' };
  if (Number.isNaN(concurrencyLimit) || concurrencyLimit < 0) return { ok: false, error: '并发上限应为非负整数' };
  if (!isValidProxyUrl(form.proxyUrl)) return { ok: false, error: '代理 URL 需以 http://、https:// 或 socks5:// 开头' };
  return {
    ok: true,
    value: { groupIds: [...form.groupIds], priority, weight, concurrencyLimit, proxyUrl: form.proxyUrl.trim() },
  };
}

export function isHttpUrl(v: string): boolean {
  try {
    const u = new URL(v.trim());
    return u.protocol === 'http:' || u.protocol === 'https:';
  } catch {
    return false;
  }
}
