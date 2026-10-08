import type { ImportCodexInput, ImportCodexItem } from './api/types';

/** 一份待导入的 auth.json（粘贴框或上传的文件）。 */
export interface AuthJsonSource {
  /** 结果与错误提示里显示的来源：「粘贴内容」或文件名。 */
  label: string;
  name?: string;
  text: string;
}

export interface ImportOptions {
  groupIds: number[];
  priority: number;
  concurrencyLimit: number;
  proxyUrl: string;
}

export const MAX_AUTH_JSON_BYTES = 1024 * 1024;

/** 「alice.json」→「alice」作账号名；默认文件名 auth.json 不带信息量，交给服务端取邮箱。 */
export function nameFromFilename(filename: string): string | undefined {
  const base = filename.replace(/\.json$/i, '').trim();
  if (!base || base.toLowerCase() === 'auth') return undefined;
  return base;
}

/**
 * 本地预检 `~/.codex/auth.json`：JSON 对象，含 tokens（订阅登录）或 OPENAI_API_KEY（API Key 登录）。
 * 返回错误信息，合法返回 null；深层校验交给服务端。
 */
export function validateAuthJson(text: string): string | null {
  const trimmed = text.trim();
  if (!trimmed) return '内容为空';
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return '不是合法的 JSON';
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return '内容应为 JSON 对象';
  const obj = parsed as Record<string, unknown>;
  const hasTokens = !!obj.tokens && typeof obj.tokens === 'object';
  const hasApiKey = typeof obj.OPENAI_API_KEY === 'string' && obj.OPENAI_API_KEY.trim() !== '';
  if (!hasTokens && !hasApiKey) return '缺少 tokens 字段（或 OPENAI_API_KEY），请确认是 Codex CLI 的 auth.json';
  return null;
}

/** POST /api/admin/accounts/import-codex 的请求体；authJson 为原文（去掉首尾空白）。 */
export function buildImportCodexBody(sources: AuthJsonSource[], opts: ImportOptions): ImportCodexInput {
  return {
    items: sources.map((s) => {
      const item: ImportCodexItem = { authJson: s.text.trim() };
      const name = s.name?.trim();
      if (name) item.name = name;
      return item;
    }),
    groupIds: [...opts.groupIds],
    priority: opts.priority,
    concurrencyLimit: opts.concurrencyLimit,
    proxyUrl: opts.proxyUrl.trim(),
  };
}
