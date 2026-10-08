import type { TokenPair } from './types';

/** 接口错误：HTTP 状态 + 契约里的 error.code / error.message。status=0 表示网络错误。 */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly body: unknown;

  constructor(status: number, code: string, message: string, body: unknown = null) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.body = body;
  }
}

export interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export type QueryValue = string | number | boolean | null | undefined;
export type Query = { [key: string]: QueryValue };

export interface RequestOptions {
  query?: Query;
  body?: unknown;
  headers?: Record<string, string>;
  signal?: AbortSignal;
  /** false：不带 Bearer，401 也不触发刷新（登录、刷新、公开接口）。 */
  auth?: boolean;
}

export interface ApiClientOptions {
  baseUrl?: string;
  fetch?: typeof fetch;
  storage?: KeyValueStorage | null;
  /** 用 navigator.locks 跨标签页串行化刷新（浏览器里默认开启）。 */
  crossTabLock?: boolean;
}

export const REFRESH_TOKEN_KEY = 'forge-admin.refreshToken';
const REFRESH_LOCK_NAME = 'forge-admin.refresh';
export const SESSION_EXPIRED_MESSAGE = '登录已过期，请重新登录';

function defaultStorage(): KeyValueStorage | null {
  try {
    return typeof window !== 'undefined' && window.localStorage ? window.localStorage : null;
  } catch {
    return null;
  }
}

function statusMessage(status: number): string {
  switch (status) {
    case 400:
      return '请求参数错误';
    case 401:
      return '未登录或登录已过期';
    case 403:
      return '没有权限执行该操作';
    case 404:
      return '资源不存在';
    case 409:
      return '数据冲突，请刷新后重试';
    case 413:
      return '请求内容过大';
    case 429:
      return '请求过于频繁，请稍后再试';
    case 501:
      return '服务端尚未实现该功能';
    default:
      return status >= 500 ? `服务器错误（HTTP ${status}）` : `请求失败（HTTP ${status}）`;
  }
}

/** 把非 2xx 响应解析成 ApiError（兼容 {error:{code,message}} 与网关的 OpenAI 风格错误）。 */
export async function errorFromResponse(res: Response): Promise<ApiError> {
  let body: unknown = null;
  try {
    const text = await res.text();
    if (text) {
      try {
        body = JSON.parse(text);
      } catch {
        body = text;
      }
    }
  } catch {
    body = null;
  }
  let code = `HTTP_${res.status}`;
  let message = statusMessage(res.status);
  const err = body && typeof body === 'object' ? (body as { error?: unknown }).error : undefined;
  if (err && typeof err === 'object') {
    const e = err as { code?: unknown; message?: unknown };
    if (typeof e.code === 'string' && e.code) code = e.code;
    if (typeof e.message === 'string' && e.message) message = e.message;
  }
  return new ApiError(res.status, code, message, body);
}

export function filenameFromDisposition(header: string | null): string | null {
  if (!header) return null;
  const star = /filename\*\s*=\s*(?:UTF-8'[^']*')?([^;]+)/i.exec(header);
  if (star?.[1]) {
    try {
      return decodeURIComponent(star[1].trim().replace(/^"|"$/g, ''));
    } catch {
      // 回落到普通 filename=
    }
  }
  const plain = /filename\s*=\s*"?([^";]+)"?/i.exec(header);
  return plain?.[1] ? plain[1].trim() : null;
}

/**
 * 管理后台 HTTP 客户端。
 * - access token 只在内存；refresh token 在 localStorage。
 * - 401 时单飞刷新一次再重试；刷新失败清会话并通知 onAuthFailure 监听者（跳登录页）。
 *   服务端对「用已轮换掉的 refresh token」判定为复用并吊销整个会话，所以并发刷新必须合并。
 */
export class ApiClient {
  private readonly baseUrl: string;
  private readonly fetchImpl: typeof fetch;
  private readonly storage: KeyValueStorage | null;
  private readonly crossTabLock: boolean;
  private accessToken: string | null = null;
  private refreshing: Promise<boolean> | null = null;
  private readonly authFailureListeners = new Set<() => void>();

  constructor(opts: ApiClientOptions = {}) {
    this.baseUrl = (opts.baseUrl ?? '').replace(/\/+$/, '');
    this.fetchImpl = opts.fetch ?? ((input, init) => globalThis.fetch(input, init));
    this.storage = opts.storage === undefined ? defaultStorage() : opts.storage;
    this.crossTabLock = opts.crossTabLock ?? true;
  }

  getAccessToken(): string | null {
    return this.accessToken;
  }

  getRefreshToken(): string | null {
    try {
      return this.storage?.getItem(REFRESH_TOKEN_KEY) ?? null;
    } catch {
      return null;
    }
  }

  setSession(tokens: Pick<TokenPair, 'accessToken' | 'refreshToken'>): void {
    this.accessToken = tokens.accessToken;
    this.writeRefreshToken(tokens.refreshToken);
  }

  clearSession(): void {
    this.accessToken = null;
    this.writeRefreshToken(null);
  }

  /** 会话失效（刷新失败）时回调；返回取消订阅函数。 */
  onAuthFailure(listener: () => void): () => void {
    this.authFailureListeners.add(listener);
    return () => {
      this.authFailureListeners.delete(listener);
    };
  }

  url(path: string, query?: Query): string {
    let qs = '';
    if (query) {
      const params = new URLSearchParams();
      for (const [key, value] of Object.entries(query)) {
        if (value === undefined || value === null || value === '') continue;
        params.append(key, String(value));
      }
      qs = params.toString();
    }
    return `${this.baseUrl}${path}${qs ? `?${qs}` : ''}`;
  }

  async request<T>(method: string, path: string, opts: RequestOptions = {}): Promise<T> {
    const res = await this.send(method, path, opts);
    if (!res.ok) throw await errorFromResponse(res);
    if (res.status === 204) return undefined as T;
    const text = await res.text();
    if (!text) return undefined as T;
    try {
      return JSON.parse(text) as T;
    } catch {
      return text as unknown as T;
    }
  }

  /** 下载类接口（CSV 等）：同样走鉴权与刷新。 */
  async requestBlob(
    method: string,
    path: string,
    opts: RequestOptions = {},
  ): Promise<{ blob: Blob; filename: string | null }> {
    const res = await this.send(method, path, { ...opts, headers: { Accept: '*/*', ...opts.headers } });
    if (!res.ok) throw await errorFromResponse(res);
    return { blob: await res.blob(), filename: filenameFromDisposition(res.headers.get('Content-Disposition')) };
  }

  /** 单飞刷新：并发调用共享同一次 /auth/refresh。resolve(false) = refresh token 无效。 */
  refresh(): Promise<boolean> {
    if (!this.refreshing) {
      this.refreshing = this.withRefreshLock(() => this.doRefresh()).finally(() => {
        this.refreshing = null;
      });
    }
    return this.refreshing;
  }

  private async send(method: string, path: string, opts: RequestOptions): Promise<Response> {
    if (opts.auth === false) return this.rawFetch(method, path, opts, null);

    const sentWith = this.accessToken;
    const first = await this.rawFetch(method, path, opts, sentWith);
    if (first.status !== 401) return first;

    // 请求在途期间别处已刷新过 → 直接用新 token 重试，不再发起刷新。
    const renewed = (this.accessToken !== null && this.accessToken !== sentWith) || (await this.refresh());
    if (!renewed) {
      this.expireSession();
      throw new ApiError(401, 'SESSION_EXPIRED', SESSION_EXPIRED_MESSAGE);
    }
    const retry = await this.rawFetch(method, path, opts, this.accessToken);
    if (retry.status === 401) this.expireSession();
    return retry;
  }

  private async doRefresh(): Promise<boolean> {
    const refreshToken = this.getRefreshToken();
    if (!refreshToken) return false;
    const res = await this.rawFetch('POST', '/api/v1/auth/refresh', { body: { refreshToken } }, null);
    if (res.ok) {
      const pair = (await res.json()) as Partial<TokenPair> | null;
      if (!pair?.accessToken || !pair.refreshToken) return false;
      this.setSession({ accessToken: pair.accessToken, refreshToken: pair.refreshToken });
      return true;
    }
    if (res.status === 400 || res.status === 401 || res.status === 403) return false;
    // 5xx / 429 / 网络类问题：会话可能仍有效，不登出，把错误抛给调用方。
    throw await errorFromResponse(res);
  }

  private withRefreshLock<T>(fn: () => Promise<T>): Promise<T> {
    const locks: LockManager | undefined =
      this.crossTabLock && typeof navigator !== 'undefined' ? navigator.locks : undefined;
    if (!locks || typeof locks.request !== 'function') return fn();
    return locks.request(REFRESH_LOCK_NAME, () => fn()) as Promise<T>;
  }

  private expireSession(): void {
    this.clearSession();
    for (const listener of [...this.authFailureListeners]) listener();
  }

  private writeRefreshToken(token: string | null): void {
    try {
      if (token) this.storage?.setItem(REFRESH_TOKEN_KEY, token);
      else this.storage?.removeItem(REFRESH_TOKEN_KEY);
    } catch {
      // 存储不可用（隐私模式等）：只保留内存会话。
    }
  }

  private async rawFetch(
    method: string,
    path: string,
    opts: RequestOptions,
    token: string | null,
  ): Promise<Response> {
    const headers: Record<string, string> = { Accept: 'application/json', ...opts.headers };
    let body: string | undefined;
    if (opts.body !== undefined) {
      headers['Content-Type'] = 'application/json';
      body = JSON.stringify(opts.body);
    }
    if (token && !headers.Authorization) headers.Authorization = `Bearer ${token}`;
    try {
      return await this.fetchImpl(this.url(path, opts.query), {
        method,
        headers,
        body,
        signal: opts.signal,
        credentials: 'same-origin',
      });
    } catch (err) {
      if (err instanceof DOMException && err.name === 'AbortError') throw err;
      throw new ApiError(0, 'NETWORK_ERROR', '无法连接服务器，请检查网络或服务状态', err);
    }
  }
}

export const apiClient = new ApiClient();

export function errorMessage(err: unknown): string {
  if (err instanceof ApiError || err instanceof Error) return err.message || '未知错误';
  if (typeof err === 'string') return err;
  return '未知错误';
}
