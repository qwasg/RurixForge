import { useCallback, useEffect, useRef, useState } from 'react';
import { ArrowUpRight, Check, ChevronDown, Clock3, ExternalLink, Loader2, LogOut, RefreshCw, ShieldCheck, X } from 'lucide-react';
import { bindGlmKey, cancelOfficialLogin, channelDefaults, codexChannelStatus, getOfficialChannel,
  loginOfficialChannel, logoutOfficialChannel, type ChannelStatus } from '@/lib/channelApi';
import { postCodexInstall, postCodexLogin, postCodexLoginCancel, postCodexLogout } from '@/lib/forgeApi';
import { officialAuthUrl, reserveOfficialAuth, type OfficialChannel } from '@/lib/officialAuth';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import { channelBrands as brands, ChannelBrandHeader, QuotaMeter } from './ChannelBrand';
import AntigravityConnectionCard from './AntigravityConnectionCard';
import './channelConnections.css';

export function OfficialChannelCard({ channel }: { channel: OfficialChannel }) {
  const brand = brands[channel];
  const [status, setStatus] = useState<ChannelStatus>(channelDefaults[channel]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loginInfo, setLoginInfo] = useState<ChannelStatus['login']>();
  const [afterInstall, setAfterInstall] = useState(false);
  const [keyForm, setKeyForm] = useState(false);
  const [apiKey, setApiKey] = useState('');
  const [model, setModel] = useState(channelDefaults[channel].model);
  const [choice, setChoice] = useState('');
  const request = useRef<Promise<void> | null>(null);
  const operationInFlight = useRef(false);
  const sequence = useRef(0);
  const mounted = useRef(true);
  const authWindow = useRef<ReturnType<typeof reserveOfficialAuth> | null>(null);
  const wasConnected = useRef(false);
  const selectedModel = useChatStore((s) => s.selectedModelId);
  const sessionId = useSessionStore((s) => s.activeSessionId);
  const draftEngine = useSessionStore((s) => s.draftAgentEngine);
  const activeEngine = useSessionStore((s) => s.sessions.find((session) => session.id === s.activeSessionId)?.agentEngine);
  const active = (sessionId ? activeEngine : draftEngine) === (channel === 'codex' ? 'codex' : 'local')
    && selectedModel === (choice || status.modelId);

  const refresh = useCallback((usage = false) => {
    if (request.current) return request.current;
    const version = sequence.current;
    const task = (async () => {
      try {
        const next = channel === 'codex' ? await codexChannelStatus() : await getOfficialChannel(channel, usage);
        if (!mounted.current || version !== sequence.current) return;
        setStatus(next);
        if (channel === 'antigravity' && next.login?.state === 'pending') setLoginInfo(next.login);
        setError(next.error ?? next.install?.error ?? next.login?.error ?? null);
        if (next.configured && !wasConnected.current) {
          void useChatStore.getState().ensureModels(true, channel === 'codex' ? 'codex' : undefined);
        }
        wasConnected.current = next.configured;
      } catch (caught) {
        if (mounted.current && version === sequence.current) setError(caught instanceof Error ? caught.message : String(caught));
      } finally { if (mounted.current && version === sequence.current) setLoading(false); }
    })();
    request.current = task;
    void task.finally(() => { if (request.current === task) request.current = null; });
    return task;
  }, [channel]);

  useEffect(() => {
    mounted.current = true;
    void refresh(true);
    const focus = () => { void refresh(true); };
    window.addEventListener('focus', focus);
    return () => {
      mounted.current = false;
      sequence.current++;
      request.current = null;
      window.removeEventListener('focus', focus);
    };
  }, [refresh]);
  useEffect(() => { if (!keyForm) setModel(status.model); }, [keyForm, status.model]);

  const pending = !!loginInfo && !status.configured && loginInfo.state !== 'key_required'
    && !['failed', 'cancelled', 'authenticated'].includes(status.login?.state ?? '');
  useEffect(() => {
    if (!pending && !afterInstall && !status.install?.running) return;
    const timer = window.setInterval(() => { if (!busy) void refresh(); }, 3000);
    return () => window.clearInterval(timer);
  }, [afterInstall, busy, pending, refresh, status.install?.running]);
  useEffect(() => {
    if (status.configured && loginInfo && channel !== 'glm') {
      setLoginInfo(undefined);
      authWindow.current?.close();
      void refresh(true);
    }
    if (status.install?.error || status.login?.state === 'failed') {
      setAfterInstall(false);
      setLoginInfo(undefined);
      authWindow.current?.close();
    }
  }, [channel, loginInfo, refresh, status.configured, status.install?.error, status.login?.state]);

  const run = async (operation: string, action: () => Promise<void>) => {
    if (operationInFlight.current) return;
    operationInFlight.current = true;
    sequence.current++;
    setBusy(operation);
    setError(null);
    try {
      if (request.current) await request.current;
      await action();
      await refresh(true);
    } catch (caught) {
      const message = caught instanceof Error ? caught.message : String(caught);
      if (mounted.current) setError(message);
      authWindow.current?.close();
      setAfterInstall(false);
      setLoginInfo(undefined);
      useToastStore.getState().push('error', message);
    } finally { operationInFlight.current = false; if (mounted.current) setBusy(null); }
  };

  const login = (resume = false, device = false) => {
    if (operationInFlight.current) return Promise.resolve();
    if (!resume) authWindow.current = reserveOfficialAuth(channel);
    return run('login', async () => {
      if (channel === 'codex' && !status.installed) {
        await postCodexInstall();
        setAfterInstall(true);
        setLoginInfo({ state: 'installing' });
        return;
      }
      const raw = channel === 'codex' ? await postCodexLogin({ kind: device ? 'deviceCode' : 'chatgpt' }) : await loginOfficialChannel(channel);
      const url = raw.authUrl ?? ('verificationUrl' in raw ? raw.verificationUrl : undefined);
      const info = { ...raw, state: raw.state === 'installing' ? 'installing' : channel === 'glm' ? 'key_required' : 'pending',
        authUrl: typeof url === 'string' ? url : undefined } as NonNullable<ChannelStatus['login']>;
      setLoginInfo(info);
      if (channel !== 'codex') setStatus((s) => ({ ...s, login: info }));
      if (info.state === 'installing') { setAfterInstall(true); return; }
      if (info.authUrl) {
        if (!officialAuthUrl(channel, info.authUrl)) {
          if (channel === 'codex') await postCodexLoginCancel(info.loginId);
          else if (channel === 'kimi' || channel === 'antigravity') await cancelOfficialLogin(channel);
          throw new Error('官方授权地址校验失败，请重试');
        }
        if (!await authWindow.current?.navigate(info.authUrl)) useToastStore.getState().push('warning', '请点击卡片中的官方授权链接继续登录');
      } else { authWindow.current?.close(); }
      if (channel === 'glm') setKeyForm(true);
    });
  };
  useEffect(() => {
    if (afterInstall && status.installed && !status.install?.running && !busy) {
      setAfterInstall(false);
      void login(true);
    }
  }, [afterInstall, status.installed, status.install?.running, busy]);

  const useModel = () => run('model', async () => {
    const engine = channel === 'codex' ? 'codex' : 'local';
    const sessions = useSessionStore.getState();
    const id = choice || status.modelId;
    if (!id) throw new Error('官方模型目录暂未就绪，请刷新后重试');
    if (sessions.activeSessionId) await sessions.setAgentEngine(sessions.activeSessionId, engine);
    else sessions.setDraftAgentEngine(engine);
    const updated = useSessionStore.getState();
    const appliedEngine = updated.activeSessionId
      ? updated.sessions.find((session) => session.id === updated.activeSessionId)?.agentEngine
      : updated.draftAgentEngine;
    if (appliedEngine !== engine) throw new Error('切换 Agent 引擎失败，请重试');
    await useChatStore.getState().ensureModels(true, channel === 'codex' ? 'codex' : undefined);
    await useChatStore.getState().pickModel(id);
    if (useChatStore.getState().selectedModelId !== id) throw new Error('切换调用模型失败，请重试');
    useToastStore.getState().push('success', `已切换到 ${brand.title}`);
  });
  const safeUrl = officialAuthUrl(channel, loginInfo?.authUrl);
  const quotaUrl = officialAuthUrl(channel, status.quotaUrl);
  const badge = loading ? '连接中' : status.install?.running || afterInstall ? '安装中' : pending ? '等待授权'
    : status.configured ? channel === 'glm' ? '已绑定' : '已授权' : status.cloudMode ? 'Forge 云' : '未授权';
  const disabled = !!busy || loading || pending || afterInstall || status.install?.running;
  const quotaWindows = channel === 'antigravity'
    ? status.quota.windows.filter((window) => window.id === (choice || status.modelId).replace(/^antigravity[:/]/, '')).slice(0, 1)
    : status.quota.windows;
  const visibleWindows = channel === 'antigravity' && !quotaWindows.length ? status.quota.windows.slice(0, 2) : quotaWindows;

  return <article className={`channel-card channel-${channel}`} data-testid={`channel-card-${channel}`} aria-label={`${brand.title} 渠道`}>
    <ChannelBrandHeader channel={channel} badge={badge} connected={status.configured} />
    <div className="channel-card-body">
      <div className="channel-identity"><div><span className="channel-eyebrow">{status.configured ? '已连接账户' : '官方订阅'}</span><div className="channel-account">{status.account?.email || status.account?.name || (status.configured ? channel === 'glm' ? 'Coding Plan Key 已绑定' : channel === 'antigravity' ? 'Google 账户已授权' : '官方 CLI 授权已连接' : '连接你的订阅账户')}</div></div>{status.account?.planType && <span className="channel-plan">{status.account.planType}</span>}</div>
      {visibleWindows.length > 0 ? <div className="channel-quota-grid">{visibleWindows.map((bucket) => <QuotaMeter key={bucket.id} window={bucket} />)}</div>
        : <div className="channel-quota-empty"><Clock3 size={16} /><div><strong>{loading ? '读取渠道状态…' : !status.configured ? channel === 'glm' ? '在官方控制台查看套餐额度' : '授权后查看额度' : channel === 'glm' ? '在官方控制台查看套餐额度' : '官方暂未返回额度'}</strong><span>{status.quota.error ?? (channel === 'glm' ? 'Coding Plan 订阅额度由智谱官方管理' : '读取官方用量与重置时间')}</span></div></div>}
      {(status.quota.stale || status.quota.updatedAt) && <div className="channel-data-time">{status.quota.stale ? '缓存数据 · 点击刷新获取最新额度' : `官方数据 · ${new Date(status.quota.updatedAt!).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}`}</div>}
      <div className="channel-model-row"><span className="channel-eyebrow">调用模型</span>{status.models && status.models.length > 0 ? <select aria-label={`${brand.title} 调用模型`} value={choice || status.modelId} onChange={(event) => setChoice(event.target.value)}>{status.models.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}</select> : <span className="channel-model-name">{status.model}</span>}</div>
      {error && <div role="alert" className="channel-error">{error}</div>}
      {safeUrl && !status.configured && <div className="channel-auth-info"><span>{loginInfo?.userCode && <code>{loginInfo.userCode}</code>}</span><a href={safeUrl} target="_blank" rel="noopener noreferrer">继续官方授权<ExternalLink size={11} /></a></div>}
      {keyForm && channel === 'glm' && <form className="channel-key-form" onSubmit={(event) => { event.preventDefault(); void run('bind', async () => { const next = await bindGlmKey(apiKey.trim(), model.trim()); setStatus(next); setApiKey(''); setKeyForm(false); setLoginInfo(undefined); }); }}>
        <label htmlFor="glm-subscription-key">Coding Plan Key</label><input id="glm-subscription-key" type="password" autoComplete="off" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder="在官方登录页获取套餐 Key" />
        <label htmlFor="glm-subscription-model">模型 ID</label><input id="glm-subscription-model" value={model} onChange={(event) => setModel(event.target.value)} placeholder="glm-5.3" />
        <div className="channel-form-actions"><button type="submit" className="channel-primary" disabled={!apiKey.trim() || !!busy}>绑定订阅</button><button type="button" className="channel-text-button" onClick={() => { setKeyForm(false); setApiKey(''); }}>取消</button></div><small>密钥保存于本机加密凭证库。</small>
      </form>}
      <div className="channel-actions">
        {status.configured ? <button type="button" className={`channel-primary${active ? ' channel-active' : ''}`} disabled={!!busy || !status.modelId} onClick={() => void useModel()}>{active ? <Check size={14} /> : <ArrowUpRight size={14} />}{active ? '当前使用' : '使用模型'}</button>
          : <button type="button" className="channel-primary" disabled={disabled} onClick={() => void login()} data-testid={`channel-login-${channel}`}>{busy || status.install?.running || afterInstall ? <Loader2 size={14} className="animate-spin" /> : <ArrowUpRight size={14} />}{afterInstall || status.install?.running ? channel === 'antigravity' ? '正在准备连接组件' : '正在安装官方 CLI' : pending ? '等待官方授权' : !status.installed && channel !== 'glm' ? channel === 'antigravity' ? '准备并网页授权' : '安装并官方授权' : brand.login}</button>}
        {pending || afterInstall ? <button type="button" className="channel-icon-button" aria-label={`取消 ${brand.title} 登录`} disabled={!!busy} onClick={() => void run('cancel', async () => { if (!afterInstall) { if (channel === 'codex') await postCodexLoginCancel(loginInfo?.loginId); else await cancelOfficialLogin(channel); } setAfterInstall(false); setLoginInfo(undefined); authWindow.current?.close(); })}><X size={14} /></button>
          : <button type="button" className="channel-icon-button" aria-label={`刷新 ${brand.title} 额度`} disabled={!!busy || loading} onClick={() => void refresh(true)}><RefreshCw size={13} className={loading ? 'animate-spin' : ''} /></button>}
        {status.configured && <button type="button" className="channel-icon-button" aria-label={`退出 ${brand.title}`} disabled={!!busy} onClick={() => void run('logout', async () => { if (channel === 'codex') await postCodexLogout(); else await logoutOfficialChannel(channel); setLoginInfo(undefined); })}><LogOut size={13} /></button>}
      </div>
      <div className="channel-footnote"><ShieldCheck size={11} /><span>{brand.note}</span></div>
      <div className="channel-links">{channel === 'antigravity' && <AntigravityConnectionCard advancedOnly />}{quotaUrl && <a href={quotaUrl} target="_blank" rel="noopener noreferrer">官方额度<ExternalLink size={10} /></a>}{channel === 'codex' && !status.configured && status.installed && <button type="button" disabled={disabled} onClick={() => void login(false, true)}>设备码授权</button>}{channel === 'glm' && <button type="button" disabled={!!busy} onClick={() => setKeyForm(!keyForm)}>{status.configured ? '更新套餐 Key' : '已有套餐 Key'}</button>}</div>
    </div>
  </article>;
}

export default function ChannelConnections() {
  const [expanded, setExpanded] = useState(() => {
    try { return localStorage.getItem('forge:channelConnectionsExpanded') !== '0'; }
    catch { return true; }
  });
  const toggle = () => {
    const next = !expanded;
    setExpanded(next);
    try { localStorage.setItem('forge:channelConnectionsExpanded', next ? '1' : '0'); } catch { /* Optional preference. */ }
  };
  return <section className="channel-connections" aria-label="官方订阅渠道" data-testid="official-channel-connections">
    <div className="channel-section-heading"><div><h2>连接你的模型订阅</h2><p>四个渠道，随时切换。登录、模型与额度集中管理。</p></div><button type="button" className="channel-collapse" data-testid="channel-connections-toggle" aria-expanded={expanded} aria-controls="official-channel-grid" onClick={toggle}><ShieldCheck size={12} /><span>本机凭证</span><span>{expanded ? '收起' : '展开'}</span><ChevronDown size={14} className={expanded ? '' : 'is-collapsed'} /></button></div>
    <div id="official-channel-grid" className="channel-grid" hidden={!expanded}><OfficialChannelCard channel="codex" /><OfficialChannelCard channel="antigravity" /><OfficialChannelCard channel="kimi" /><OfficialChannelCard channel="glm" /></div>
  </section>;
}
