import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Check, ExternalLink, RefreshCw, TerminalSquare, X } from 'lucide-react';
import {
  getCodexAccount,
  getCodexMcpStatus,
  getCodexModels,
  getCodexRateLimits,
  getCodexStatus,
  postCodexConfig,
  postCodexInstall,
  postCodexLogin,
  postCodexLoginCancel,
  postCodexLogout,
  type CodexMcpServer,
  type CodexStatus,
} from '@/lib/forgeApi';
import { useSessionStore } from '@/lib/sessionStore';
import { useChatStore } from '@/lib/chatStore';
import { useToastStore } from '@/lib/toastStore';
import { SetCard, SetH1, SetInput, SetRow, SetSelect, SetToggle, SmBtn } from './controls';

interface ModelChoice {
  id: string;
  label: string;
}

interface LimitBucket {
  id: string;
  label: string;
  usedPercent: number;
  resetsAt?: number | string;
}

interface RuntimeMcpServer {
  name: string;
  runtimeStatus?: string;
  authStatus?: string;
}

function safeHttpUrl(value: unknown): string | null {
  if (typeof value !== 'string' || value.trim() === '') return null;
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null;
  } catch {
    return null;
  }
}

function modelsFrom(value: unknown): ModelChoice[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((raw) => {
    if (!raw || typeof raw !== 'object') return [];
    const model = raw as Record<string, unknown>;
    const id = model.id ?? model.model;
    if (typeof id !== 'string' || id === '') return [];
    const label = model.label ?? model.displayName ?? model.name;
    return [{ id, label: typeof label === 'string' && label !== '' ? label : id }];
  });
}

function bucketFrom(id: string, raw: unknown): LimitBucket | null {
  if (!raw || typeof raw !== 'object') return null;
  const value = raw as Record<string, unknown>;
  const used = value.usedPercent ?? value.used_percent;
  if (typeof used !== 'number') return null;
  const labelRaw = value.label ?? value.name;
  return {
    id,
    label: typeof labelRaw === 'string' ? labelRaw : id === 'primary' ? '主要额度' : id === 'secondary' ? '次要额度' : id,
    usedPercent: Math.max(0, Math.min(100, used)),
    resetsAt: typeof value.resetsAt === 'number' || typeof value.resetsAt === 'string'
      ? value.resetsAt
      : undefined,
  };
}

function limitsFrom(raw: unknown): LimitBucket[] {
  if (!raw || typeof raw !== 'object') return [];
  const value = raw as Record<string, unknown>;
  const ordered = ['primary', 'secondary'];
  const direct = ordered.flatMap((key) => {
    const bucket = bucketFrom(key, value[key]);
    return bucket ? [bucket] : [];
  });
  if (direct.length > 0) return direct;
  const byId = value.rateLimitsByLimitId;
  if (!byId || typeof byId !== 'object') return [];
  return Object.entries(byId as Record<string, unknown>).flatMap(([limitId, group]) => {
    if (!group || typeof group !== 'object') return [];
    return ordered.flatMap((key) => {
      const bucket = bucketFrom(`${limitId}-${key}`, (group as Record<string, unknown>)[key]);
      return bucket ? [{ ...bucket, label: `${limitId} · ${key === 'primary' ? '主要额度' : '次要额度'}` }] : [];
    });
  });
}

function resetLabel(value: number | string | undefined): string {
  if (value === undefined) return '';
  const date = new Date(typeof value === 'number' ? value * 1000 : value);
  if (Number.isNaN(date.getTime())) return '';
  return `重置 ${date.toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })}`;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function mcpAvailable(server: { available?: boolean; enabled?: boolean; present?: boolean }): boolean {
  return server.available ?? (server.enabled !== false && server.present !== false);
}

function Availability({ ok, text }: { ok: boolean; text: string }) {
  return (
    <span className="flex items-center gap-1.5 text-[11px] text-fg-3">
      <span className={`h-1.5 w-1.5 rounded-full ${ok ? 'bg-dot-done' : 'bg-dot-idle'}`} />
      {text}
    </span>
  );
}

function RateLimitBar({ bucket }: { bucket: LimitBucket }) {
  const remaining = Math.max(0, Math.round(100 - bucket.usedPercent));
  return (
    <div data-testid={`codex-limit-${bucket.id}`} className="flex flex-col gap-1.5 px-4 py-3">
      <div className="flex items-center text-[11.5px]">
        <span className="min-w-0 flex-1 text-fg-2">{bucket.label}</span>
        <span className="font-code text-fg-3">剩余 {remaining}%</span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-shell-active">
        <div className="h-full rounded-full bg-sage" style={{ width: `${remaining}%` }} />
      </div>
      {resetLabel(bucket.resetsAt) && <div className="text-[10px] text-fg-4">{resetLabel(bucket.resetsAt)}</div>}
    </div>
  );
}

/** Codex 安装、认证、额度、模型和 MCP 的单一设置入口。 */
export default function CodexPage() {
  const sessionId = useSessionStore((state) => state.activeSessionId);
  const hydrateAgentDefaults = useSessionStore((state) => state.hydrateAgentDefaults);
  const [status, setStatus] = useState<CodexStatus | null>(null);
  const [models, setModels] = useState<ModelChoice[]>([]);
  const [mcp, setMcp] = useState<CodexMcpServer[]>([]);
  const [mcpLive, setMcpLive] = useState(false);
  const [mcpRuntime, setMcpRuntime] = useState<RuntimeMcpServer[]>([]);
  const [mcpRuntimeError, setMcpRuntimeError] = useState<string | null>(null);
  const [binPath, setBinPath] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [loginInfo, setLoginInfo] = useState<Record<string, unknown> | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refreshSequence = useRef(0);
  const refreshRequest = useRef(0);
  const refreshInFlight = useRef<Promise<void> | null>(null);
  const lastSharedAuthMode = useRef<string | null | undefined>(undefined);
  const accountReady = typeof status?.account?.authMode === 'string' && status.account.authMode !== '';

  const refresh = useCallback(async () => {
    const request = refreshRequest.current + 1;
    refreshRequest.current = request;
    const inFlight = refreshInFlight.current;
    if (inFlight) {
      await inFlight;
      // 同一次慢请求期间可能来了多个定时 tick，只让最后一个补拉一次。
      if (request !== refreshRequest.current) return;
    }
    const sequence = refreshSequence.current + 1;
    refreshSequence.current = sequence;
    const task = (async () => {
      try {
        const [next, mcpResult] = await Promise.all([
          getCodexStatus(sessionId),
          getCodexMcpStatus(sessionId).catch(() => null),
        ]);
        let nextModels = modelsFrom(next.models);
        if (nextModels.length === 0) {
          const response = await getCodexModels().catch(() => ({ ok: false, models: [] }));
          nextModels = modelsFrom(response.models);
        }
        const [freshAccount, freshLimits] = next.installed
          ? await Promise.all([
              getCodexAccount().catch(() => null),
              getCodexRateLimits().catch(() => null),
            ])
          : [null, null];
        if (sequence !== refreshSequence.current) return;
        const mergedAccount = {
          ...next.account,
          ...(freshAccount ?? {}),
          rateLimits: freshLimits?.rateLimits ?? freshAccount?.rateLimits ?? next.account.rateLimits,
        };
        const mergedStatus = { ...next, account: mergedAccount };
        setStatus(mergedStatus);
        useChatStore.setState({
          codexAccount: mergedAccount,
          codexRateLimits: freshLimits?.rateLimits ?? null,
        });
        const authMode =
          typeof mergedAccount.authMode === 'string' ? mergedAccount.authMode : null;
        if (lastSharedAuthMode.current !== authMode) {
          lastSharedAuthMode.current = authMode;
          void useChatStore.getState().ensureModels(true, 'codex');
        }
        let nextError: string | null = null;
        if (typeof mergedAccount.lastError === 'string' && mergedAccount.lastError.trim() !== '') {
          nextError = mergedAccount.lastError;
          setLoginInfo(null);
        } else if (typeof next.install?.error === 'string' && next.install.error.trim() !== '') {
          nextError = next.install.error;
        }
        setError(nextError);
        setBinPath(mergedStatus.config?.codexBin ?? '');
        setMcp(mcpResult?.servers ?? next.mcp?.servers ?? []);
        setMcpLive(mcpResult?.live === true);
        setMcpRuntime(
          Array.isArray(mcpResult?.runtime?.data)
            ? mcpResult.runtime.data.flatMap((raw) => {
                const item = raw;
                if (typeof item.name !== 'string' || item.name === '') return [];
                return [{
                  name: item.name,
                  runtimeStatus: typeof item.runtimeStatus === 'string' ? item.runtimeStatus : undefined,
                  authStatus: typeof item.authStatus === 'string' ? item.authStatus : undefined,
                }];
              })
            : [],
        );
        setMcpRuntimeError(
          typeof mcpResult?.runtimeError === 'string' ? mcpResult.runtimeError : null,
        );
        hydrateAgentDefaults(mergedStatus.config?.defaultEngine);
        setModels(nextModels);
      } catch (caught) {
        if (sequence !== refreshSequence.current) return;
        setError(errorText(caught));
      }
    })();
    refreshInFlight.current = task;
    try {
      await task;
    } finally {
      if (refreshInFlight.current === task) refreshInFlight.current = null;
    }
  }, [hydrateAgentDefaults, sessionId]);

  useEffect(() => {
    void refresh();
    return () => {
      refreshSequence.current += 1;
      refreshRequest.current += 1;
    };
  }, [refresh]);

  useEffect(() => {
    if (!status?.install?.running) return;
    const timer = window.setInterval(() => void refresh(), 2000);
    return () => window.clearInterval(timer);
  }, [refresh, status?.install?.running]);

  useEffect(() => {
    if (!loginInfo || accountReady) return;
    const timer = window.setInterval(() => void refresh(), 3000);
    return () => window.clearInterval(timer);
  }, [accountReady, loginInfo, refresh]);

  const run = async (key: string, action: () => Promise<void>) => {
    setBusy(key);
    setError(null);
    try {
      await action();
    } catch (caught) {
      const message = errorText(caught);
      setError(message);
      useToastStore.getState().push('error', `Codex 操作失败:${message}`);
    } finally {
      setBusy(null);
    }
  };

  const configure = (patch: Parameters<typeof postCodexConfig>[0]) =>
    run('config', async () => {
      // 配置提交与后台轮询可能并发；先作废旧快照，避免它在 POST
      // 成功后又把界面回盖成保存前的值。最后再串行拉一次服务端事实。
      refreshSequence.current += 1;
      const next = await postCodexConfig(patch);
      setStatus(next);
      hydrateAgentDefaults(next.config?.defaultEngine);
      await refresh();
    });

  const login = (kind: 'chatgpt' | 'deviceCode' | 'apiKey') =>
    run(`login-${kind}`, async () => {
      setError(null);
      setLoginInfo(null);
      const response = await postCodexLogin({ kind, ...(kind === 'apiKey' ? { apiKey } : {}) });
      setLoginInfo(response);
      if (typeof response.authUrl === 'string' && response.authUrl !== '') {
        const authUrl = safeHttpUrl(response.authUrl);
        if (!authUrl) {
          const responseLoginId = typeof response.loginId === 'string' ? response.loginId : undefined;
          await postCodexLoginCancel(responseLoginId).catch(() => undefined);
          setLoginInfo(null);
          throw new Error('登录地址不是安全的 HTTP(S) URL，已阻止打开');
        }
        const opened = window.open(authUrl, '_blank', 'noopener,noreferrer');
        if (opened === null) {
          useToastStore.getState().push('warning', '浏览器阻止了登录窗口，请使用下方链接继续');
        }
      }
      if (kind === 'apiKey') setApiKey('');
      await refresh();
    });

  const limits = useMemo(() => limitsFrom(status?.account?.rateLimits), [status?.account?.rateLimits]);
  const userCode = loginInfo?.userCode ?? loginInfo?.user_code ?? loginInfo?.deviceCode;
  const verificationUri = loginInfo?.verificationUri ?? loginInfo?.verificationUrl ?? loginInfo?.verification_uri ?? loginInfo?.authUrl;
  const safeVerificationUri = safeHttpUrl(verificationUri);
  const loginId = typeof loginInfo?.loginId === 'string' ? loginInfo.loginId : undefined;
  const version =
    status && typeof (status as CodexStatus & { version?: unknown }).version === 'string'
      ? (status as CodexStatus & { version: string }).version
      : '';

  return (
    <div data-testid="settings-page-codex" className="flex flex-col">
      <div className="flex items-center gap-2">
        <SetH1>Codex</SetH1>
        <button
          type="button"
          aria-label="刷新 Codex 状态"
          data-testid="codex-refresh"
          onClick={() => void refresh()}
          className="mb-4 flex h-6 w-6 items-center justify-center rounded-md text-fg-3 hover:bg-shell-hover"
        >
          <RefreshCw size={12} />
        </button>
      </div>
      <div className="flex flex-col gap-3">
        {error && (
          <div data-testid="codex-error" className="rounded-lg border border-edge bg-warn-bg px-3 py-2 text-[11px] text-warn">
            {error}
          </div>
        )}

        <SetCard testId="codex-install-card">
          <SetRow
            title="Codex 运行时"
            desc={`${status?.command || '由 Forge 托管 @openai/codex 与 Computer Use'}${version ? ` · ${version}` : ''}`}
            control={
              <Availability
                ok={status?.installed === true}
                text={status?.installed ? (status.running ? '已安装 · 运行中' : '已安装') : '未安装'}
              />
            }
          />
          <SetRow
            title="托管安装"
            desc={status?.install?.running ? '正在安装，请保持 Forge 打开' : '安装或更新 Codex 与 open-computer-use'}
            control={
              <SmBtn
                testId="codex-install"
                accent={!status?.installed}
                disabled={busy !== null || status?.install?.running === true}
                label={status?.install?.running ? '安装中…' : status?.installed ? '重新安装' : '安装'}
                onClick={() =>
                  void run('install', async () => {
                    await postCodexInstall();
                    await refresh();
                  })
                }
              />
            }
          />
          <SetRow
            title="指定 Codex 路径"
            desc="留空时依次使用 PATH 与 Forge 托管版本"
            last
            control={
              <div className="flex items-center gap-1.5">
                <SetInput value={binPath} onChange={setBinPath} width={250} testId="codex-bin-path" placeholder="codex 或 codex.exe 路径" />
                <SmBtn
                  testId="codex-bin-save"
                  label="保存"
                  disabled={busy !== null}
                  onClick={() => void configure({ codexBin: binPath.trim() })}
                />
              </div>
            }
          />
          {status?.install?.log && (
            <pre className="max-h-28 overflow-auto border-t border-edge px-4 py-2 font-code text-[10px] whitespace-pre-wrap text-fg-4">
              {status.install.log}
            </pre>
          )}
        </SetCard>

        <SetCard testId="codex-account-card">
          <SetRow
            title="账户"
            desc={accountReady ? status?.account?.email || `认证方式：${status?.account?.authMode}` : '登录后可使用 Codex 模型与额度'}
            control={
              accountReady ? (
                <div className="flex items-center gap-2">
                  {status?.account?.planType && <span className="rounded-full bg-acc-bg px-2 py-0.5 text-[10px] text-acc">{status.account.planType}</span>}
                  <SmBtn
                    testId="codex-logout"
                    label="退出登录"
                    disabled={busy !== null}
                    onClick={() =>
                      void run('logout', async () => {
                        await postCodexLogout();
                        setLoginInfo(null);
                        await refresh();
                      })
                    }
                  />
                </div>
              ) : (
                <SmBtn
                  testId="codex-login-chatgpt"
                  label={<><ExternalLink size={11} />ChatGPT 登录</>}
                  accent
                  disabled={busy !== null || status?.installed !== true}
                  onClick={() => void login('chatgpt')}
                />
              )
            }
          />
          {!accountReady && (
            <>
              <SetRow
                title="设备码登录"
                desc="浏览器登录不可用时使用设备码流程"
                control={
                  <SmBtn testId="codex-login-device" label="获取设备码" disabled={busy !== null || status?.installed !== true} onClick={() => void login('deviceCode')} />
                }
              />
              <SetRow
                title="API Key"
                desc="密钥仅发送到本机 agentd，不会回显"
                last
                control={
                  <div className="flex items-center gap-1.5">
                    <SetInput value={apiKey} onChange={setApiKey} type="password" width={230} testId="codex-api-key" placeholder="sk-…" />
                    <SmBtn testId="codex-login-api-key" label="登录" disabled={busy !== null || apiKey.trim() === '' || status?.installed !== true} onClick={() => void login('apiKey')} />
                  </div>
                }
              />
            </>
          )}
          {(typeof userCode === 'string' || typeof verificationUri === 'string') && (
            <div data-testid="codex-device-code" className="flex items-center gap-2 border-t border-edge px-4 py-2.5 text-[11px] text-fg-2">
              <TerminalSquare size={12} className="text-fg-3" />
              {typeof userCode === 'string' && <span className="font-code text-fg">{userCode}</span>}
              {typeof verificationUri === 'string' && (
                safeVerificationUri ? (
                  <a
                    href={safeVerificationUri}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="min-w-0 truncate text-acc hover:underline"
                  >
                    {verificationUri}
                  </a>
                ) : (
                  <span className="min-w-0 truncate text-fg-3">{verificationUri}</span>
                )
              )}
            </div>
          )}
          {!accountReady && loginInfo && (
            <div className="flex items-center justify-end gap-2 border-t border-edge px-4 py-2">
              <span className="min-w-0 flex-1 text-[10.5px] text-fg-4">等待 Codex 完成登录…</span>
              <SmBtn
                testId="codex-login-cancel"
                label="取消登录"
                disabled={busy !== null}
                onClick={() =>
                  void run('login-cancel', async () => {
                    await postCodexLoginCancel(loginId);
                    setLoginInfo(null);
                  })
                }
              />
            </div>
          )}
        </SetCard>

        {limits.length > 0 && (
          <SetCard testId="codex-limits-card">
            <SetRow title="额度" desc="来自 Codex 账户的当前使用窗口" last />
            <div className="divide-y divide-edge border-t border-edge">
              {limits.map((bucket) => <RateLimitBar key={bucket.id} bucket={bucket} />)}
            </div>
          </SetCard>
        )}

        <SetCard testId="codex-config-card">
          <SetRow
            title="新会话默认引擎"
            desc="已有会话仍可在 Composer 单独切换"
            control={
              <SetSelect
                testId="codex-default-engine"
                value={status?.config?.defaultEngine === 'codex' ? 'codex' : 'local'}
                options={[{ value: 'local', label: '本地' }, { value: 'codex', label: 'Codex' }]}
                onPick={(defaultEngine) => void configure({ defaultEngine })}
              />
            }
          />
          <SetRow
            title="默认模型"
            desc="新建 Codex 会话使用的模型"
            control={
              <SetSelect
                testId="codex-default-model"
                value={status?.config?.defaultModel ?? ''}
                options={[
                  { value: '', label: '自动' },
                  ...models.map((model) => ({ value: model.id, label: model.label })),
                ]}
                onPick={(defaultModel) => void configure({ defaultModel })}
              />
            }
          />
          <SetRow
            title="自动注册项目 MCP"
            desc="为 Codex 线程注入 Forge 项目工具"
            control={
              <SetToggle
                testId="codex-auto-mcp"
                on={status?.config?.autoRegisterMcp !== false}
                onChange={(autoRegisterMcp) => void configure({ autoRegisterMcp })}
              />
            }
          />
          <SetRow
            title="Computer Use"
            desc="启用 open-computer-use；写操作仍遵循会话权限"
            last
            control={
              <SetToggle
                testId="codex-computer-use"
                on={status?.config?.computerUse === true}
                onChange={(computerUse) => void configure({ computerUse })}
              />
            }
          />
        </SetCard>

        <SetCard testId="codex-mcp-card">
          <SetRow title="MCP 服务" desc={mcpLive ? '当前 Codex 线程实时状态' : '当前项目静态注册状态'} last />
          <div className="border-t border-edge">
            {mcp.length === 0 && mcpRuntime.length === 0 ? (
              <div className="px-4 py-3 text-[11px] text-fg-4">暂无 MCP 状态</div>
            ) : (
              [...mcp, ...mcpRuntime
                .filter((runtime) => !mcp.some((server) => server.name === runtime.name))
                .map((runtime) => ({ name: runtime.name, command: '', available: true }))]
                .map((server, index) => {
                const runtime = mcpRuntime.find((item) => item.name === server.name);
                const runtimeReady = runtime
                  ? runtime.runtimeStatus === 'connected' && runtime.authStatus !== 'notLoggedIn'
                  : true;
                const ok = mcpAvailable(server) && runtimeReady;
                const liveLabel = [runtime?.runtimeStatus, runtime?.authStatus].filter(Boolean).join(' · ');
                return (
                  <div
                    key={server.name}
                    data-testid={`codex-mcp-${server.name}`}
                    className={`flex items-center gap-2 px-4 py-2 text-[11px] ${index > 0 ? 'border-t border-edge' : ''}`}
                  >
                    {ok ? <Check size={11} className="text-sage" /> : <X size={11} className="text-fg-4" />}
                    <span className="min-w-0 flex-1 truncate font-code text-fg-2">{server.name}</span>
                    <span className="max-w-[280px] truncate font-code text-[9.5px] text-fg-4">{liveLabel || server.command || (ok ? 'ready' : 'unavailable')}</span>
                  </div>
                );
              })
            )}
            {mcpRuntimeError && <div className="border-t border-edge px-4 py-2 text-[10px] text-warn">{mcpRuntimeError}</div>}
          </div>
        </SetCard>
      </div>
    </div>
  );
}
