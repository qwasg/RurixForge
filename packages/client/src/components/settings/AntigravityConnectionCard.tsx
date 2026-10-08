import { useCallback, useEffect, useRef, useState } from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { ArrowUpRight, Check, Clock3, ExternalLink, Loader2, RefreshCw, Settings2, ShieldCheck, X } from 'lucide-react';
import { getAntigravityStatus, postAntigravityConfig, postAntigravityProbe, type AntigravityQuota, type AntigravityStatus } from '@/lib/forgeApi';
import type { QuotaWindow } from '@/lib/channelApi';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import { ChannelBrandHeader, QuotaMeter } from './ChannelBrand';

const EMPTY: AntigravityStatus = { configured: false, keyConfigured: false, baseUrl: '', model: 'gemini-3.8-flash', availability: 'needs-config' };

export function antigravityQuotaWindows(quota?: AntigravityQuota | null): QuotaWindow[] {
  return (['primary', 'secondary'] as const).flatMap((id) => {
    const bucket = quota?.[id];
    if (!bucket) return [];
    const used = typeof bucket.usedPercent === 'number' ? bucket.usedPercent
      : typeof bucket.remainingPercent === 'number' ? 100 - bucket.remainingPercent : NaN;
    if (!Number.isFinite(used)) return [];
    const duration = bucket.windowDurationMins;
    return [{ id, label: typeof duration === 'number' && duration > 0
      ? duration < 60 ? `${duration} 分钟` : duration < 1440 ? `${duration / 60} 小时` : `${duration / 1440} 天`
      : id === 'primary' ? '主要额度' : '次要额度', usedPercent: Math.max(0, Math.min(100, used)), resetsAt: bucket.resetsAt }];
  });
}

export default function AntigravityConnectionCard({ advancedOnly = false }: { advancedOnly?: boolean } = {}) {
  const [status, setStatus] = useState<AntigravityStatus>(EMPTY);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [baseUrl, setBaseUrl] = useState('');
  const [model, setModel] = useState(EMPTY.model);
  const [key, setKey] = useState('');
  const [choice, setChoice] = useState('');
  const mounted = useRef(true);
  const generation = useRef(0);
  const operation = useRef(false);
  const request = useRef<Promise<void> | null>(null);
  const selectedModel = useChatStore((s) => s.selectedModelId);
  const sessionId = useSessionStore((s) => s.activeSessionId);
  const draftEngine = useSessionStore((s) => s.draftAgentEngine);
  const activeEngine = useSessionStore((s) => s.sessions.find((session) => session.id === s.activeSessionId)?.agentEngine);
  const modelId = choice && choice !== status.model ? `antigravity:${choice}` : 'antigravity';
  const active = (sessionId ? activeEngine : draftEngine) === 'local' && selectedModel === modelId;
  const connected = status.configured && status.availability !== 'disconnected' && status.availability !== 'offline';

  const refresh = useCallback(() => {
    if (request.current) return request.current;
    const version = generation.current;
    const task = (async () => {
      try {
        const next = await getAntigravityStatus();
        if (!mounted.current || version !== generation.current) return;
        setStatus(next);
        setError(next.lastError ?? null);
      } catch (caught) {
        if (mounted.current && version === generation.current) setError(caught instanceof Error ? caught.message : String(caught));
      } finally { if (mounted.current && version === generation.current) setLoading(false); }
    })();
    request.current = task;
    void task.finally(() => { if (request.current === task) request.current = null; });
    return task;
  }, []);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    const onFocus = () => { if (!operation.current) void refresh(); };
    window.addEventListener('focus', onFocus);
    return () => { mounted.current = false; generation.current++; request.current = null; window.removeEventListener('focus', onFocus); };
  }, [refresh]);

  const run = async (action: () => Promise<void>) => {
    if (operation.current) return;
    operation.current = true;
    generation.current++;
    setBusy(true); setError(null);
    try {
      if (request.current) await request.current;
      await action();
    } catch (caught) {
      const message = caught instanceof Error ? caught.message : String(caught);
      if (mounted.current) setError(message);
      useToastStore.getState().push('error', message);
    } finally { operation.current = false; if (mounted.current) { setBusy(false); setLoading(false); } }
  };

  const configure = () => { setBaseUrl(status.baseUrl); setModel(status.model || EMPTY.model); setKey(''); setError(null); setOpen(true); };
  const closeForm = (next: boolean) => { if (busy) return; setOpen(next); if (!next) setKey(''); };
  const probe = async (draft = false) => {
    const result = await postAntigravityProbe(draft ? { baseUrl: baseUrl.trim(), model: model.trim(), key: key.trim() || undefined } : undefined);
    if (draft) {
      if (!result.ok) throw new Error(result.error || '反重力连接失败');
      useToastStore.getState().push('success', `反重力连接正常 · ${result.latencyMs ?? 0} ms`);
    } else {
      // The probe writes the persistent status even on a failed connection.
      await refresh();
      if (!result.ok) throw new Error(result.error || '反重力连接失败');
    }
  };
  const save = () => run(async () => {
    const url = new URL(baseUrl.trim());
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error('请输入有效的反代服务地址');
    const payload = { baseUrl: url.href.replace(/\/$/, ''), model: model.trim(), enabled: true, ...(key.trim() ? { key: key.trim() } : {}) };
    const result = await postAntigravityConfig(payload);
    if (!result.configured) throw new Error('请填写反代访问密钥完成连接');
    setKey(''); setChoice(''); setOpen(false);
    await refresh();
    await useChatStore.getState().ensureModels(true);
    useToastStore.getState().push('success', '反重力渠道已连接');
  });
  const useModel = () => run(async () => {
    const sessions = useSessionStore.getState();
    if (sessions.activeSessionId) await sessions.setAgentEngine(sessions.activeSessionId, 'local');
    else sessions.setDraftAgentEngine('local');
    const updated = useSessionStore.getState();
    const engine = updated.activeSessionId ? updated.sessions.find((session) => session.id === updated.activeSessionId)?.agentEngine : updated.draftAgentEngine;
    if (engine !== 'local') throw new Error('切换 Agent 引擎失败，请重试');
    await useChatStore.getState().ensureModels(true);
    await useChatStore.getState().pickModel(modelId);
    if (useChatStore.getState().selectedModelId !== modelId) throw new Error('切换反重力模型失败，请重试');
    useToastStore.getState().push('success', '已切换到 Antigravity');
  });
  const windows = antigravityQuotaWindows(status.rateLimits ?? status.quota);
  const badge = loading ? '连接中' : !status.configured ? '未连接' : connected ? '已连接' : '连接异常';
  const options = Array.from(new Set([status.model, ...(status.models ?? [])].filter(Boolean)));

  return <>
    {advancedOnly ? <button type="button" className="channel-text-button" onClick={configure}>反代配置</button> : <article className="channel-card channel-antigravity" data-testid="channel-card-antigravity" aria-label="Antigravity 反重力渠道">
      <ChannelBrandHeader channel="antigravity" badge={badge} connected={connected} />
      <div className="channel-card-body">
        <div className="channel-identity"><div><span className="channel-eyebrow">Google AI 订阅</span><div className="channel-account" title={status.baseUrl}>{status.configured ? '反重力反代已连接' : '连接你的反重力订阅'}</div></div>{status.latencyMs != null && <span className="channel-plan">{status.latencyMs} ms</span>}</div>
        {windows.length ? <div className="channel-quota-grid">{windows.map((bucket) => <QuotaMeter key={bucket.id} window={bucket} />)}</div>
          : <div className="channel-quota-empty"><Clock3 size={15} /><div><strong>{loading ? '读取渠道状态…' : status.configured ? '反代暂未返回额度' : '连接后同步额度'}</strong><span>{status.configured ? '点击刷新，获取用量与重置时间' : 'Google AI Pro / Ultra'}</span></div></div>}
        {status.lastProbedAt && <div className="channel-data-time">最近同步 · {new Date(status.lastProbedAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}</div>}
        <div className="channel-model-row"><span className="channel-eyebrow">调用模型</span>{options.length > 1 ? <select aria-label="Antigravity 调用模型" value={choice || status.model} onChange={(event) => setChoice(event.target.value)}>{options.map((name) => <option key={name} value={name}>{name}</option>)}</select> : <span className="channel-model-name" title={status.model}>{status.model}</span>}</div>
        {error && !open && <div className="channel-error" role="alert">{error}</div>}
        <div className="channel-actions">
          <button type="button" className={`channel-primary${active ? ' channel-active' : ''}`} disabled={busy || loading || (status.configured && !connected)} onClick={() => connected ? void useModel() : configure()} data-testid="channel-login-antigravity">{busy ? <Loader2 size={14} className="animate-spin" /> : active ? <Check size={14} /> : <ArrowUpRight size={14} />}{connected ? active ? '当前使用' : '使用模型' : '连接反重力'}</button>
          <button type="button" className="channel-icon-button" disabled={busy || loading} aria-label="刷新 Antigravity 额度" onClick={() => { if (status.configured) void run(() => probe()); else void refresh(); }}><RefreshCw size={13} className={busy ? 'animate-spin' : ''} /></button>
          {status.configured && <button type="button" className="channel-icon-button" disabled={busy} aria-label="配置 Antigravity" onClick={configure}><Settings2 size={13} /></button>}
        </div>
        <div className="channel-footnote"><ShieldCheck size={11} /><span>本机反代 · 凭证加密保存</span></div>
        <div className="channel-links"><a href="https://antigravity.google/" target="_blank" rel="noopener noreferrer">官方入口<ExternalLink size={10} /></a><a href="https://antigravity.google/docs/cli/commands/usage" target="_blank" rel="noopener noreferrer">官方额度<ExternalLink size={10} /></a></div>
      </div>
    </article>}
    <Dialog.Root open={open} onOpenChange={closeForm}>
      <Dialog.Portal><Dialog.Overlay className="channel-dialog-overlay" /><Dialog.Content className="channel-dialog" data-testid="antigravity-connection-dialog">
        <div className="channel-dialog-heading"><img src="/channel-logos/antigravity.svg" alt="Google Antigravity 官方图标" width={32} height={32} /><div><Dialog.Title>连接反重力</Dialog.Title><Dialog.Description>关联已登录的反重力反代服务，使用 Google AI 订阅模型。</Dialog.Description></div><Dialog.Close className="channel-icon-button" aria-label="关闭反重力配置" disabled={busy}><X size={15} /></Dialog.Close></div>
        <a className="channel-setup-link" href="https://antigravity.google/docs/cli/install/" target="_blank" rel="noopener noreferrer"><ExternalLink size={14} /><span>打开官方 CLI 安装与登录指南</span><ArrowUpRight size={14} /></a>
        <form className="channel-key-form" onSubmit={(event) => { event.preventDefault(); void save(); }}>
          <label htmlFor="antigravity-connection-url">反代服务地址</label><input id="antigravity-connection-url" type="url" value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} placeholder="http://127.0.0.1:8080" required />
          <label htmlFor="antigravity-connection-model">模型 ID</label><input id="antigravity-connection-model" value={model} onChange={(event) => setModel(event.target.value)} placeholder="gemini-3.8-flash" required />
          <label htmlFor="antigravity-connection-key">反代访问密钥</label><input id="antigravity-connection-key" type="password" autoComplete="off" value={key} onChange={(event) => setKey(event.target.value)} placeholder={status.keyConfigured ? '已保存，留空保留原密钥' : '填写本机反代服务的访问密钥'} required={!status.keyConfigured} />
          {error && <div className="channel-error" role="alert">{error}</div>}
          <div className="channel-form-actions"><button type="button" className="channel-secondary" disabled={busy || !baseUrl.trim() || !model.trim() || (!key.trim() && !status.keyConfigured)} onClick={() => void run(() => probe(true))}>测试连接</button><button type="submit" className="channel-primary" disabled={busy || !baseUrl.trim() || !model.trim() || (!key.trim() && !status.keyConfigured)}>{busy ? <Loader2 size={14} className="animate-spin" /> : <Check size={14} />}保存并连接</button></div>
          <small>Google 登录由官方 CLI 或反代服务完成。此处只保存反代连接信息。</small>
        </form>
      </Dialog.Content></Dialog.Portal>
    </Dialog.Root>
  </>;
}
