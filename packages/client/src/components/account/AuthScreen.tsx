import { useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { ArrowUpRight, ChevronDown, ChevronRight, CircleAlert, CircleCheck, KeyRound, Server, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { errorMessage, postAccountConfig, postEmailCode } from '@/lib/accountApi';
import { gateForced, gateVisible, useAccountStore } from '@/lib/accountStore';
import { bridge, isDesktopBridge } from '@/lib/bridge';
import ForgeMark from '@/components/ForgeMark';
import { useSettingsStore } from '@/lib/settingsStore';
import { useOverlayStore } from '@/lib/overlayStore';

/**
 * 云账户登录门(D-041,15 §8.2):`!loggedIn && !byoConfigured && !devMock` 时强制显示,
 * 或由「登录」入口手动打开(此时可关闭)。注册字段随 auth-config:invite 模式要邀请码、
 * 开了邮箱验证且 SMTP 可用时要验证码(60s 冷却)、closed 模式不开放注册。
 *
 * D-046 版式(Claude 式,配色仍是 forge 中性底):宽屏左表单列(品牌组合 + 衬线大标题 + 表单卡)
 * + 右品牌面板(折带斜条 + folded 品牌标擦出入场);窄屏只留表单列,标题上方补品牌标。
 * 桌面 overlay 形态下系统三钮画在右上角:品牌面板贴右上缘且同为 --bg-sunk(TitleBar 下发的三钮底色),
 * 顶部留拖拽区;窄屏时表单列头部让出三钮宽度。
 */

type Tab = 'login' | 'register';

export const EMAIL_CODE_COOLDOWN_S = 60;

const inputCls =
  'h-10 w-full rounded-xl border border-edge-strong bg-shell-input px-3.5 text-[13.5px] text-fg outline-none transition-[border-color,box-shadow] placeholder:text-fg-4 hover:border-fg-4 focus:border-acc focus:ring-[3px] focus:ring-acc-ring';

const secondaryBtnCls =
  'h-10 shrink-0 rounded-xl border border-edge-strong bg-shell-panel px-3.5 text-[12.5px] text-fg-2 transition-colors hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-60';

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="flex items-baseline justify-between text-[12.5px] font-medium text-fg-2">
        {label}
        {hint && <span className="text-[11px] font-normal text-fg-4">{hint}</span>}
      </span>
      {children}
    </label>
  );
}

function PrimaryButton({ busy, label, testId }: { busy: boolean; label: string; testId: string }) {
  return (
    <button
      type="submit"
      data-testid={testId}
      disabled={busy}
      aria-busy={busy}
      className="mt-1.5 flex h-10 w-full items-center justify-center gap-2 rounded-xl bg-fg text-[13.5px] font-medium text-shell-bg transition-opacity hover:opacity-90 disabled:cursor-wait disabled:opacity-90"
    >
      {busy && <ForgeMark busy size={14} />}
      {busy ? '请稍候…' : label}
    </button>
  );
}

function FormError({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <div
      data-testid="auth-error"
      role="alert"
      className="flex items-start gap-2 rounded-xl bg-danger-bg px-3.5 py-2.5 text-[12.5px] leading-[18px] text-danger"
    >
      <CircleAlert size={14} className="mt-0.5 shrink-0" />
      <span className="min-w-0">{message}</span>
    </div>
  );
}

const EMAIL_RE = /^[^\s@]+@[^\s@]+$/;

function LoginForm() {
  const login = useAccountStore((st) => st.login);
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    if (!EMAIL_RE.test(email.trim())) return setError('请输入有效的邮箱地址');
    if (password === '') return setError('请输入密码');
    setBusy(true);
    setError(null);
    try {
      const status = await login(email.trim(), password);
      if (!status?.loggedIn) setError(status?.lastError ?? '登录未完成,请稍后重试');
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form data-testid="auth-login-form" className="flex flex-col gap-4" onSubmit={(e) => void submit(e)} noValidate>
      <Field label="邮箱">
        <input
          data-testid="auth-email"
          type="email"
          autoComplete="email"
          autoFocus
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="you@example.com"
          className={inputCls}
        />
      </Field>
      <Field label="密码">
        <input
          data-testid="auth-password"
          type="password"
          autoComplete="current-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className={inputCls}
        />
      </Field>
      <FormError message={error} />
      <PrimaryButton busy={busy} label="登录" testId="auth-submit" />
    </form>
  );
}

function RegisterForm() {
  const register = useAccountStore((st) => st.register);
  const authConfig = useAccountStore((st) => st.authConfig);
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [nickname, setNickname] = useState('');
  const [inviteCode, setInviteCode] = useState('');
  const [emailCode, setEmailCode] = useState('');
  const [cooldown, setCooldown] = useState(0);
  const [sending, setSending] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const mode = authConfig?.registrationMode ?? 'open';
  const needInvite = mode === 'invite';
  const needEmailCode = authConfig?.requireEmailVerify === true && authConfig.smtpEnabled === true;

  useEffect(() => {
    if (cooldown <= 0) return;
    const t = setTimeout(() => setCooldown((c) => c - 1), 1000);
    return () => clearTimeout(t);
  }, [cooldown]);

  if (mode === 'closed') {
    return (
      <div data-testid="auth-register-closed" className="flex flex-col items-center gap-1.5 py-8 text-center">
        <span className="font-serif text-[17px] font-semibold text-fg">暂未开放注册</span>
        <span className="text-[12px] text-fg-3">请联系管理员获取账号,或使用已有账号登录</span>
      </div>
    );
  }

  const sendCode = async () => {
    if (sending || cooldown > 0) return;
    if (!EMAIL_RE.test(email.trim())) return setError('请先填写有效的邮箱地址');
    setSending(true);
    setError(null);
    try {
      await postEmailCode({ email: email.trim(), purpose: 'register' });
      setCooldown(EMAIL_CODE_COOLDOWN_S);
      setNotice('验证码已发送,请查收邮件');
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSending(false);
    }
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    if (!EMAIL_RE.test(email.trim())) return setError('请输入有效的邮箱地址');
    if (password.length < 8) return setError('密码至少 8 位');
    if (needInvite && inviteCode.trim() === '') return setError('请输入邀请码');
    if (needEmailCode && emailCode.trim() === '') return setError('请输入邮箱验证码');
    setBusy(true);
    setError(null);
    try {
      const status = await register({
        email: email.trim(),
        password,
        ...(nickname.trim() !== '' ? { nickname: nickname.trim() } : {}),
        ...(needInvite ? { inviteCode: inviteCode.trim() } : {}),
        ...(needEmailCode ? { emailCode: emailCode.trim() } : {}),
      });
      if (!status?.loggedIn) setError(status?.lastError ?? '注册未完成,请稍后重试');
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form data-testid="auth-register-form" className="flex flex-col gap-4" onSubmit={(e) => void submit(e)} noValidate>
      <Field label="邮箱">
        <input
          data-testid="auth-email"
          type="email"
          autoComplete="email"
          autoFocus
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="you@example.com"
          className={inputCls}
        />
      </Field>
      {needEmailCode && (
        <Field label="邮箱验证码">
          <div className="flex gap-2">
            <input
              data-testid="auth-email-code"
              inputMode="numeric"
              autoComplete="one-time-code"
              value={emailCode}
              onChange={(e) => setEmailCode(e.target.value)}
              className={inputCls}
            />
            <button
              type="button"
              data-testid="auth-send-code"
              disabled={sending || cooldown > 0}
              onClick={() => void sendCode()}
              className={secondaryBtnCls}
            >
              {cooldown > 0 ? `${cooldown} 秒后重发` : sending ? '发送中…' : '发送验证码'}
            </button>
          </div>
        </Field>
      )}
      <Field label="密码" hint="至少 8 位">
        <input
          data-testid="auth-password"
          type="password"
          autoComplete="new-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className={inputCls}
        />
      </Field>
      <Field label="昵称" hint="可选">
        <input
          data-testid="auth-nickname"
          maxLength={32}
          value={nickname}
          onChange={(e) => setNickname(e.target.value)}
          className={inputCls}
        />
      </Field>
      {needInvite && (
        <Field label="邀请码">
          <input
            data-testid="auth-invite"
            value={inviteCode}
            onChange={(e) => setInviteCode(e.target.value)}
            className={inputCls}
          />
        </Field>
      )}
      {notice && !error && (
        <div className="flex items-center gap-2 rounded-xl bg-sage-bg px-3.5 py-2.5 text-[12.5px] text-sage">
          <CircleCheck size={14} className="shrink-0" />
          {notice}
        </div>
      )}
      <FormError message={error} />
      <PrimaryButton busy={busy} label="注册并登录" testId="auth-submit" />
    </form>
  );
}

function ServerAddress({ extra }: { extra?: ReactNode }) {
  const status = useAccountStore((st) => st.status);
  const refreshStatus = useAccountStore((st) => st.refreshStatus);
  const loadAuthConfig = useAccountStore((st) => st.loadAuthConfig);
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState(status?.serverUrl ?? '');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  const toggle = () => {
    if (!open) {
      setValue(status?.serverUrl ?? '');
      setMessage(null);
    }
    setOpen((v) => !v);
  };

  const save = async (e: FormEvent) => {
    e.preventDefault();
    const serverUrl = value.trim().replace(/\/+$/, '');
    if (busy || serverUrl === '') return;
    setBusy(true);
    setMessage(null);
    try {
      await postAccountConfig({ serverUrl });
      await Promise.all([refreshStatus(), loadAuthConfig()]);
      setMessage({ ok: true, text: '已保存服务器地址' });
    } catch (err) {
      setMessage({ ok: false, text: errorMessage(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex w-full flex-col items-center gap-3">
      <div className="flex flex-wrap items-center justify-center gap-x-2.5 gap-y-1 text-[12px] text-fg-3">
        <button
          type="button"
          data-testid="auth-server-toggle"
          aria-expanded={open}
          onClick={toggle}
          className="flex items-center gap-1.5 rounded-md transition-colors hover:text-fg"
        >
          <Server size={12} />
          服务器地址
          {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </button>
        {extra && (
          <>
            <span aria-hidden className="text-fg-4">
              ·
            </span>
            {extra}
          </>
        )}
      </div>
      {open && (
        <form className="forge-pop-in flex w-full flex-col gap-1.5" onSubmit={(e) => void save(e)}>
          <div className="flex gap-2">
            <input
              data-testid="auth-server-input"
              value={value}
              onChange={(e) => setValue(e.target.value)}
              placeholder="http://127.0.0.1:8110"
              className={cn(inputCls, 'font-code text-[12.5px]')}
            />
            <button
              type="submit"
              data-testid="auth-server-save"
              disabled={busy || value.trim() === ''}
              className={secondaryBtnCls}
            >
              {busy ? '保存中…' : '保存'}
            </button>
          </div>
          {message && (
            <span
              data-testid="auth-server-message"
              className={cn('pl-1 text-[11.5px]', message.ok ? 'text-sage' : 'text-danger')}
            >
              {message.text}
            </span>
          )}
        </form>
      )}
    </div>
  );
}

/** 品牌面板背景:logo 的 X 形折带放大成斜条(主斜带 ≈53° 两条 accent,右上臂 45° 一条中性)。 */
function RibbonStrips() {
  return (
    <svg
      aria-hidden
      viewBox="0 0 600 800"
      preserveAspectRatio="xMidYMid slice"
      className="pointer-events-none absolute inset-0 h-full w-full"
    >
      <polygon points="-40,-260 820,887 700,977 -160,-170" className="fill-acc" opacity={0.07} />
      <polygon points="182,-218 902,742 878,760 158,-200" className="fill-acc" opacity={0.12} />
      <polygon points="-60,1060 700,300 772,372 12,1132" className="fill-fg" opacity={0.035} />
    </svg>
  );
}

function BrandPanel() {
  return (
    <aside
      aria-hidden
      data-testid="auth-brand-panel"
      className="relative hidden w-[46%] max-w-[680px] shrink-0 overflow-hidden rounded-l-[28px] border-l border-edge bg-shell-sunk lg:block"
    >
      <div className="home-ambient" />
      <RibbonStrips />
      {/* 顶部拖拽区(桌面无边框窗口;浏览器忽略) */}
      <div className="absolute inset-x-0 top-0 h-9 [-webkit-app-region:drag]" />
      <div className="absolute inset-0 flex items-center justify-center pb-[14%]">
        <ForgeMark variant="folded" reveal className="h-auto w-[46%] max-w-[300px] text-fg" />
      </div>
      <div className="absolute inset-x-12 bottom-12">
        <p className="font-serif text-[26px] font-semibold leading-[1.35] tracking-tight text-fg">
          与 Agent 一起,
          <br />
          锻造游戏世界
        </p>
        <p className="mt-3 text-[13px] text-fg-3">RurixForge · AI 优先的游戏引擎工作台</p>
      </div>
    </aside>
  );
}

export default function AuthScreen() {
  const visible = useAccountStore(gateVisible);
  const forced = useAccountStore(gateForced);
  const status = useAccountStore((st) => st.status);
  const authConfig = useAccountStore((st) => st.authConfig);
  const loadAuthConfig = useAccountStore((st) => st.loadAuthConfig);
  const closeAuth = useAccountStore((st) => st.closeAuth);
  const dismissGate = useAccountStore((st) => st.dismissGateForSession);
  const [tab, setTab] = useState<Tab>('login');

  useEffect(() => {
    if (visible) void loadAuthConfig();
  }, [visible, loadAuthConfig]);

  useEffect(() => {
    if (!visible || forced) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') closeAuth();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [visible, forced, closeAuth]);

  if (!visible) return null;

  const siteName = authConfig?.siteName ?? 'RurixForge Cloud';
  const byoAllowed = status?.byoAllowed !== false;
  const unreachable = status !== null && status.reachable === false;
  const chrome = isDesktopBridge() ? bridge().win.chrome : undefined;

  return (
    <div
      data-testid="auth-screen"
      data-forced={forced ? '1' : undefined}
      role="dialog"
      aria-modal="true"
      aria-label="登录 RurixForge 云"
      className="forge-legible fixed inset-0 z-[90] flex bg-shell-bg text-fg"
    >
      <div className="relative flex min-w-0 flex-1 flex-col overflow-y-auto">
        <header
          className={cn(
            'flex h-16 shrink-0 items-center gap-2.5 px-7 [-webkit-app-region:drag]',
            chrome === 'inset' && 'pl-[88px]',
            chrome === 'overlay' && 'auth-overlay-safe',
          )}
        >
          <ForgeMark size={22} className="text-fg" />
          <span className="font-serif text-[17px] font-semibold tracking-tight text-fg">RurixForge</span>
          <span className="flex-1" />
          {!forced && (
            <button
              type="button"
              data-testid="auth-close"
              aria-label="关闭"
              onClick={closeAuth}
              className="flex h-8 w-8 items-center justify-center rounded-lg text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg [-webkit-app-region:no-drag]"
            >
              <X size={16} />
            </button>
          )}
        </header>

        <div className="forge-rise m-auto flex w-[400px] max-w-[calc(100vw-32px)] flex-col py-8">
          <div className="mb-7 flex flex-col items-center text-center">
            <ForgeMark variant="folded" reveal size={56} className="mb-6 text-fg lg:hidden" />
            <h1 data-testid="auth-title" className="font-serif text-[32px] font-semibold leading-[1.2] tracking-tight text-fg">
              {tab === 'login' ? '欢迎回来' : '创建你的账号'}
            </h1>
            <p data-testid="auth-site-name" className="mt-2.5 text-[13px] leading-[20px] text-fg-3">
              {siteName} · 登录后使用云端模型,设置与记忆随账号同步
            </p>
          </div>

          <div className="rounded-2xl border border-edge bg-shell-panel p-6 shadow-sh1">
            <div role="tablist" className="mb-5 grid grid-cols-2 gap-1 rounded-full bg-shell-sunk p-1">
              {(['login', 'register'] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  role="tab"
                  aria-selected={tab === t}
                  data-testid={`auth-tab-${t}`}
                  onClick={() => setTab(t)}
                  className={cn(
                    'h-8 rounded-full text-[13px] transition-[background-color,color,box-shadow]',
                    tab === t ? 'bg-shell-panel font-medium text-fg shadow-sh1' : 'text-fg-3 hover:text-fg-2',
                  )}
                >
                  {t === 'login' ? '登录' : '注册'}
                </button>
              ))}
            </div>
            {unreachable && (
              <div
                data-testid="auth-unreachable"
                className="mb-4 flex items-start gap-2 rounded-xl bg-warn-bg px-3.5 py-2.5 text-[12px] leading-[18px] text-warn"
              >
                <CircleAlert size={14} className="mt-0.5 shrink-0" />
                <span className="min-w-0">
                  暂时无法连接 RurixForge 云({status?.serverUrl || '未配置地址'}),请检查网络或服务器地址。
                </span>
              </div>
            )}
            {tab === 'login' ? <LoginForm /> : <RegisterForm />}
          </div>

          {byoAllowed && <button type="button" data-testid="auth-official-channels"
            onClick={() => {
              dismissGate();
              useSettingsStore.getState().setPage('models');
              useOverlayStore.getState().open('settings');
            }}
            className="mt-4 flex items-center justify-between gap-3 rounded-xl border border-edge bg-shell-panel px-4 py-3 text-left transition-colors hover:border-fg-4 hover:bg-shell-hover">
            <span><span className="block text-[12px] font-medium text-fg">连接官方模型订阅</span><span className="mt-1 block text-[10px] text-fg-3">Codex · Antigravity · Kimi Code · GLM Coding</span></span>
            <ArrowUpRight size={16} className="shrink-0 text-fg-3" />
          </button>}

          <div className="mt-6">
            <ServerAddress
              extra={
                byoAllowed ? (
                  <button
                    type="button"
                    data-testid="auth-byo"
                    onClick={dismissGate}
                    className="flex items-center gap-1.5 rounded-md transition-colors hover:text-acc"
                  >
                    <KeyRound size={12} />
                    使用自带密钥（高级）
                  </button>
                ) : null
              }
            />
          </div>
        </div>
        {/* 与头部等高的底部留白,使表单在视觉上居中 */}
        <div aria-hidden className="h-16 shrink-0" />
      </div>
      <BrandPanel />
    </div>
  );
}
