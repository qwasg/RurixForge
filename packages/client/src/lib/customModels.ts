import type { CustomApiFormat } from './forgeApi';

/**
 * D-049 自定义模型卡片页的纯函数(请求地址预览、窗口档名、预设档位)。
 * 端点与档名规则与 agentd 逐条对应(custom_models::endpoint_url / modelspec::window_label),
 * 后端为准;这里只为表单实时预览与卡片展示,不参与实发。
 */

const K = 1024;
const M = 1024 * 1024;

/** baseUrl + 格式 → 实际请求地址(写到资源路径则原样;版本段收尾只补资源名;其余补 /v1/…)。 */
export function customEndpointUrl(baseUrl: string, format: CustomApiFormat): string {
  const base = baseUrl.trim().replace(/\/+$/, '');
  if (base === '') return '';
  const resource = format === 'anthropic' ? 'messages' : 'chat/completions';
  if (base.endsWith(`/${resource}`)) return base;
  const last = base.slice(base.lastIndexOf('/') + 1);
  return /^v\d+$/.test(last) ? `${base}/${resource}` : `${base}/v1/${resource}`;
}

/**
 * tokens → 档位名(与 agentd modelspec::window_label 同口径):哪种进制能整除就用哪种,
 * 十进制优先——128000 = 128K、200000 = 200K;只有 1024 进制整除时按 1024(131072 = 128K、
 * 204800 = 200K、1048576 = 1M);≥1M 的十进制整万保留两位(1050000 = 1.05M)。
 */
export function formatWindow(tokens: number | null | undefined): string {
  if (typeof tokens !== 'number' || !Number.isFinite(tokens) || tokens <= 0) return '—';
  const n = Math.round(tokens);
  if (n >= 1_000_000) {
    if (n % 1_000_000 === 0) return `${n / 1_000_000}M`;
    if (n % M === 0) return `${n / M}M`;
    if (n % 10_000 === 0) return `${Number((n / 1_000_000).toFixed(2))}M`;
  }
  if (n % 1000 === 0) return `${n / 1000}K`;
  if (n % K === 0) return `${n / K}K`;
  return `${Math.max(1, Math.round(n / 1000))}K`;
}

export interface TokenPreset {
  label: string;
  tokens: number;
}

const preset = (label: string, tokens: number): TokenPreset => ({ label, tokens });

/** 上下文窗口预设(与 agentd 标准档同为 1024 进制)。 */
export const CONTEXT_PRESETS: TokenPreset[] = [
  preset('32K', 32 * K),
  preset('64K', 64 * K),
  preset('128K', 128 * K),
  preset('200K', 200 * K),
  preset('256K', 256 * K),
  preset('400K', 400 * K),
  preset('1M', M),
  preset('2M', 2 * M),
];

/** 最大输出预设。 */
export const OUTPUT_PRESETS: TokenPreset[] = [
  preset('4K', 4 * K),
  preset('8K', 8 * K),
  preset('16K', 16 * K),
  preset('32K', 32 * K),
  preset('64K', 64 * K),
  preset('128K', 128 * K),
];

/** 未声明窗口时 agentd 的缺省(旧单槽默认档 1M)。 */
export const DEFAULT_CONTEXT_WINDOW = M;
/** Anthropic 格式未声明最大输出时 agentd 实发的 max_tokens。 */
export const ANTHROPIC_DEFAULT_MAX_OUTPUT = 32 * K;
/** 新建模型表单的初始窗口。 */
export const NEW_MODEL_CONTEXT = 128 * K;

/** 窗口/输出上下限(与 agentd 校验一致)。 */
export const CONTEXT_MIN = K;
export const CONTEXT_MAX = 16 * M;
export const OUTPUT_MAX = M;

/** 自定义输入框(单位 K)→ tokens;空/非法 → null。 */
export function parseKiloTokens(input: string): number | null {
  const v = Number(input.trim());
  if (input.trim() === '' || !Number.isFinite(v) || v <= 0) return null;
  return Math.round(v * K);
}

/** tokens → 自定义输入框里的 K 值(整 K 显示整数)。 */
export function toKiloInput(tokens: number | null): string {
  if (tokens === null) return '';
  const k = tokens / K;
  return Number.isInteger(k) ? String(k) : k.toFixed(1);
}

/** 两种格式的说明(卡片与表单共用)。 */
export const API_FORMATS: Array<{
  id: CustomApiFormat;
  title: string;
  path: string;
  auth: string;
  hint: string;
}> = [
  {
    id: 'openai',
    title: 'OpenAI 兼容',
    path: '/v1/chat/completions',
    auth: 'Bearer 鉴权',
    hint: 'vLLM、Ollama、通义、智谱、DeepSeek 等通用接口',
  },
  {
    id: 'anthropic',
    title: 'Anthropic 兼容',
    path: '/v1/messages',
    auth: 'x-api-key 鉴权',
    hint: 'Claude 与提供 Messages 接口的中转、Kimi / GLM 编程端点',
  },
];

export function apiFormatTitle(format: CustomApiFormat): string {
  return API_FORMATS.find((f) => f.id === format)?.title ?? format;
}
