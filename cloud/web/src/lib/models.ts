import type { AdminModel, ModelCapabilities, ModelInput, ModelPool, ModelPricing, Platform, ReasoningEffort } from './api/types';
import { microsToUnits, parseIntInput, unitsToMicros } from './format';

export const REASONING_EFFORTS: ReasoningEffort[] = ['low', 'medium', 'high', 'xhigh', 'max'];

/** §11.1 用量池：模型扣哪个池的套餐内额度。 */
export const POOL_LABEL: Record<ModelPool, string> = { api: '第三方 API 池', forge: '平台模型池' };
export const POOLS: ModelPool[] = ['api', 'forge'];
/** 与 DDL 默认值一致。 */
export const DEFAULT_POOL: ModelPool = 'api';

export const PRICE_FIELDS: { key: keyof ModelPricing; label: string; short: string }[] = [
  { key: 'inputPer1M', label: '输入', short: '入' },
  { key: 'outputPer1M', label: '输出', short: '出' },
  { key: 'cacheReadPer1M', label: '缓存读', short: '读' },
  { key: 'cacheWritePer1M', label: '缓存写', short: '写' },
];

/** 表单里的单价：每 1M tokens 的额度单位（十进制字符串）。 */
export type PricingForm = Record<keyof ModelPricing, string>;

export type Result<T> = { ok: true; value: T } | { ok: false; error: string };

export function pricingToForm(p: ModelPricing | null | undefined): PricingForm {
  return {
    inputPer1M: microsToUnits(p?.inputPer1M ?? 0),
    outputPer1M: microsToUnits(p?.outputPer1M ?? 0),
    cacheReadPer1M: microsToUnits(p?.cacheReadPer1M ?? 0),
    cacheWritePer1M: microsToUnits(p?.cacheWritePer1M ?? 0),
  };
}

/** 额度单位 / 1M tokens → micros / 1M tokens。空串视为 0；负数、非法数字报错。 */
export function pricingFromForm(form: PricingForm): Result<ModelPricing> {
  const out = { inputPer1M: 0, outputPer1M: 0, cacheReadPer1M: 0, cacheWritePer1M: 0 };
  for (const { key, label } of PRICE_FIELDS) {
    const raw = form[key].trim();
    const micros = raw === '' ? 0 : unitsToMicros(raw);
    if (Number.isNaN(micros)) return { ok: false, error: `${label}单价不是合法数字` };
    if (micros < 0) return { ok: false, error: `${label}单价不能为负数` };
    out[key] = micros;
  }
  return { ok: true, value: out };
}

export const DEFAULT_CAPABILITIES: ModelCapabilities = {
  vision: false,
  reasoningEfforts: [],
  contextWindow: 128_000,
  maxOutput: 16_384,
  tools: true,
  responses: false,
};

export interface ModelForm {
  id: string;
  displayName: string;
  platform: Platform;
  upstreamModel: string;
  vision: boolean;
  tools: boolean;
  responses: boolean;
  reasoningEfforts: ReasoningEffort[];
  contextWindow: string;
  maxOutput: string;
  pricing: PricingForm;
  enabled: boolean;
  isDefault: boolean;
  sort: string;
  pool: ModelPool;
}

export function modelToForm(m: AdminModel | null): ModelForm {
  const caps = m?.capabilities ?? DEFAULT_CAPABILITIES;
  return {
    id: m?.id ?? '',
    displayName: m?.displayName ?? '',
    platform: m?.platform ?? 'openai',
    upstreamModel: m?.upstreamModel ?? '',
    vision: caps.vision,
    tools: caps.tools,
    responses: caps.responses,
    reasoningEfforts: [...(caps.reasoningEfforts ?? [])],
    contextWindow: String(caps.contextWindow ?? 0),
    maxOutput: String(caps.maxOutput ?? 0),
    pricing: pricingToForm(m?.pricing),
    enabled: m?.enabled ?? true,
    isDefault: m?.isDefault ?? false,
    sort: String(m?.sort ?? 100),
    pool: m?.pool === 'forge' ? 'forge' : DEFAULT_POOL,
  };
}

export function modelFromForm(form: ModelForm): Result<ModelInput> {
  const id = form.id.trim();
  if (!id) return { ok: false, error: '请填写模型 ID' };
  if (/\s/.test(id)) return { ok: false, error: '模型 ID 不能包含空白字符' };
  const contextWindow = parseIntInput(form.contextWindow);
  const maxOutput = parseIntInput(form.maxOutput);
  const sort = parseIntInput(form.sort);
  if (Number.isNaN(contextWindow) || contextWindow < 0) return { ok: false, error: '上下文窗口应为非负整数' };
  if (Number.isNaN(maxOutput) || maxOutput < 0) return { ok: false, error: '最大输出应为非负整数' };
  if (Number.isNaN(sort)) return { ok: false, error: '排序应为整数' };
  const pricing = pricingFromForm(form.pricing);
  if (!pricing.ok) return pricing;
  return {
    ok: true,
    value: {
      id,
      displayName: form.displayName.trim() || id,
      platform: form.platform,
      upstreamModel: form.upstreamModel.trim(),
      capabilities: {
        vision: form.vision,
        reasoningEfforts: REASONING_EFFORTS.filter((e) => form.reasoningEfforts.includes(e)),
        contextWindow,
        maxOutput,
        tools: form.tools,
        responses: form.responses,
      },
      pricing: pricing.value,
      enabled: form.enabled,
      isDefault: form.isDefault,
      sort,
      pool: form.pool,
    },
  };
}

/** 「从账号拉取模型」批量创建时的默认值：价格为 0，由运营者随后定价；用量池取 DDL 默认（api）。 */
export function defaultModelInput(
  id: string,
  platform: Platform,
  opts: { responses: boolean; enabled: boolean; sort?: number },
): ModelInput {
  return {
    id,
    displayName: id,
    platform,
    upstreamModel: '',
    capabilities: { ...DEFAULT_CAPABILITIES, reasoningEfforts: [], responses: opts.responses },
    pricing: { inputPer1M: 0, outputPer1M: 0, cacheReadPer1M: 0, cacheWritePer1M: 0 },
    enabled: opts.enabled,
    isDefault: false,
    sort: opts.sort ?? 100,
    pool: DEFAULT_POOL,
  };
}
