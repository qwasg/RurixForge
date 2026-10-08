import { useEffect, useRef, useState, type ChangeEvent } from 'react';
import { LogOut, Upload } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useToastStore } from '@/lib/toastStore';
import ForgeMark from '@/components/ForgeMark';
import {
  deleteAvatar,
  errorMessage,
  formatAmount,
  formatMoney,
  formatTime,
  normalizeUser,
  patchProfile,
  postPassword,
  postRedeem,
  putAvatar,
  type AccountStatus,
  type CloudUser,
  type RedeemResult,
  type Subscription,
} from '@/lib/accountApi';
import { useAccountStore } from '@/lib/accountStore';
import { resizeAvatar } from '@/lib/avatarImage';
import CloudAvatar from '../account/CloudAvatar';
import { SetCard, SetH1, SetInput, SetRow, SmBtn } from './controls';
import UsageDashboard, { recentUsageRange } from './UsageDashboard';

/**
 * 账户页:资料与余额并排对齐,下方为整行云端用量与 Token 活动日历。
 * 未登录只给登录入口。数据经 agentd BFF,令牌不落前端。
 * D-046 版式:资料卡头(大头像 + 衬线昵称 + 品牌标水印)、余额大数字、未登录空状态卡(folded 品牌标);
 * 共享设置控件(SetCard / SetRow / SetSectionLabel)不动,与其它设置页保持一致。
 */

function toast(kind: 'success' | 'error' | 'info', msg: string): void {
  useToastStore.getState().push(kind, msg);
}

/** 用新 User 就地更新 status(随后后台再拉一次状态兜底)。 */
function applyUser(raw: unknown): void {
  const user = normalizeUser(raw);
  const { status, setStatus, refreshStatus } = useAccountStore.getState();
  if (user && status) setStatus({ ...status, user });
  void refreshStatus();
}

function Badge({ tone, children, testId }: { tone: 'sage' | 'warn' | 'muted' | 'acc'; children: React.ReactNode; testId?: string }) {
  return (
    <span
      data-testid={testId}
      className={cn(
        'flex h-[18px] shrink-0 items-center rounded-full px-1.5 text-[10px]',
        tone === 'sage' && 'bg-sage-bg text-sage',
        tone === 'warn' && 'bg-warn-bg text-warn',
        tone === 'acc' && 'bg-acc-bg text-acc',
        tone === 'muted' && 'bg-shell-active text-fg-3',
      )}
    >
      {children}
    </span>
  );
}

// ---------- 个人资料 ----------

function ProfileSection({ user }: { user: CloudUser }) {
  const [nickname, setNickname] = useState(user.nickname);
  const [busy, setBusy] = useState<string | null>(null);
  const [pwOpen, setPwOpen] = useState(false);
  const [pw, setPw] = useState({ old: '', next: '', confirm: '' });
  const [pwError, setPwError] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  useEffect(() => setNickname(user.nickname), [user.nickname]);

  const onFile = async (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    setBusy('avatar');
    try {
      applyUser(await putAvatar(await resizeAvatar(file)));
      toast('success', '头像已更新');
    } catch (err) {
      toast('error', `头像上传失败:${errorMessage(err)}`);
    } finally {
      setBusy(null);
    }
  };

  const removeAvatar = async () => {
    setBusy('avatar');
    try {
      applyUser(await deleteAvatar());
      toast('success', '头像已删除');
    } catch (err) {
      toast('error', `删除头像失败:${errorMessage(err)}`);
    } finally {
      setBusy(null);
    }
  };

  const saveNickname = async () => {
    const v = nickname.trim();
    if (v === user.nickname || v.length > 32) return;
    setBusy('nickname');
    try {
      applyUser(await patchProfile({ nickname: v }));
      toast('success', '昵称已保存');
    } catch (err) {
      toast('error', `保存失败:${errorMessage(err)}`);
    } finally {
      setBusy(null);
    }
  };

  const savePassword = async () => {
    if (pw.old === '') return setPwError('请输入当前密码');
    if (pw.next.length < 8) return setPwError('新密码至少 8 位');
    if (pw.next !== pw.confirm) return setPwError('两次输入的新密码不一致');
    setBusy('password');
    setPwError(null);
    try {
      await postPassword({ oldPassword: pw.old, newPassword: pw.next });
      setPw({ old: '', next: '', confirm: '' });
      setPwOpen(false);
      toast('success', '密码已修改,其它设备需重新登录');
    } catch (err) {
      setPwError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  const nicknameDirty = nickname.trim() !== user.nickname;

  return (
    <SetCard testId="account-profile-card">
      <div className="relative flex items-center gap-4 overflow-hidden rounded-t-[10px] border-b border-edge px-5 py-5">
        {/* 品牌标水印:裁在卡头右上,只做氛围 */}
        <ForgeMark className="pointer-events-none absolute -right-8 -top-12 h-[180px] text-fg opacity-[0.045]" />
        <CloudAvatar user={user} size={56} />
        <div className="relative flex min-w-0 flex-1 flex-col gap-1">
          <span className="truncate font-serif text-[20px] font-semibold leading-tight text-fg">
            {user.nickname || user.email}
          </span>
          <span className="truncate text-[12px] text-fg-3">
            {user.groupName ? `${user.groupName} 分组` : '云账号'}
            {user.createdAt ? ` · 注册于 ${formatTime(user.createdAt)}` : ''}
          </span>
        </div>
        <input
          ref={fileRef}
          type="file"
          accept="image/png,image/jpeg,image/webp,image/gif"
          data-testid="account-avatar-file"
          className="hidden"
          onChange={(e) => void onFile(e)}
        />
        <div className="relative flex shrink-0 items-center gap-1.5">
          <SmBtn
            label={<><Upload size={11} />{busy === 'avatar' ? '处理中…' : '上传头像'}</>}
            testId="account-avatar-upload"
            disabled={busy !== null}
            onClick={() => fileRef.current?.click()}
          />
          {user.hasAvatar && (
            <SmBtn label="删除" testId="account-avatar-delete" disabled={busy !== null} onClick={() => void removeAvatar()} />
          )}
        </div>
      </div>
      <SetRow
        title="昵称"
        desc="最多 32 字"
        control={
          <div className="flex items-center gap-1.5">
            <SetInput value={nickname} onChange={setNickname} width={160} testId="account-nickname-input" placeholder="未设置" />
            <SmBtn
              label="保存"
              testId="account-nickname-save"
              disabled={busy !== null || !nicknameDirty || nickname.trim().length > 32}
              onClick={() => void saveNickname()}
            />
          </div>
        }
      />
      <SetRow
        title="邮箱"
        desc="登录账号,暂不支持修改"
        control={<span data-testid="account-email" className="font-code text-[12px] text-fg-2">{user.email}</span>}
      />
      <SetRow
        title="密码"
        desc="修改后其它设备需重新登录"
        last={!pwOpen}
        control={
          <SmBtn
            label={pwOpen ? '取消' : '修改密码'}
            testId="account-password-toggle"
            onClick={() => {
              setPwOpen((v) => !v);
              setPwError(null);
              setPw({ old: '', next: '', confirm: '' });
            }}
          />
        }
      />
      {pwOpen && (
        <div data-testid="account-password-form" className="flex flex-col gap-2 px-4 py-3">
          <SetInput type="password" value={pw.old} onChange={(v) => setPw((p) => ({ ...p, old: v }))} placeholder="当前密码" width={260} testId="account-password-old" />
          <SetInput type="password" value={pw.next} onChange={(v) => setPw((p) => ({ ...p, next: v }))} placeholder="新密码(至少 8 位)" width={260} testId="account-password-new" />
          <SetInput type="password" value={pw.confirm} onChange={(v) => setPw((p) => ({ ...p, confirm: v }))} placeholder="再次输入新密码" width={260} testId="account-password-confirm" />
          {pwError && <span data-testid="account-password-error" className="text-[11.5px] text-danger">{pwError}</span>}
          <div>
            <SmBtn accent label={busy === 'password' ? '保存中…' : '确认修改'} testId="account-password-save" disabled={busy !== null} onClick={() => void savePassword()} />
          </div>
        </div>
      )}
    </SetCard>
  );
}

// ---------- 余额与套餐 ----------

function QuotaBar({ label, used, total, currency, testId }: { label: string; used: number; total: number; currency: string; testId?: string }) {
  const pct = total > 0 ? Math.min(100, Math.max(0, (used / total) * 100)) : 0;
  return (
    <div data-testid={testId} className="flex flex-col gap-1">
      <div className="flex items-center text-[11px]">
        <span className="min-w-0 flex-1 text-fg-3">{label}</span>
        <span className="font-code text-fg-3">
          {formatMoney(used, currency)} / {formatMoney(total, currency)}
        </span>
      </div>
      <div className="h-2 overflow-hidden rounded-full bg-shell-active">
        <div
          className={cn('h-full rounded-full', pct >= 100 ? 'bg-danger' : pct >= 80 ? 'bg-warn' : 'bg-sage')}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

const SUB_STATUS: Record<string, string> = { active: '生效中', expired: '已过期', cancelled: '已取消' };

function SubscriptionRow({ sub, currency }: { sub: Subscription; currency: string }) {
  return (
    <div data-testid={`account-sub-${sub.id}`} className="flex flex-col gap-2 border-b border-edge px-4 py-3">
      <div className="flex items-center gap-2 text-[12.5px]">
        <span className="font-medium text-fg">{sub.planName}</span>
        <Badge tone={sub.status === 'active' ? 'sage' : 'muted'}>{SUB_STATUS[sub.status] ?? sub.status}</Badge>
        <span className="flex-1" />
        <span className="text-[11px] text-fg-4">到期 {formatTime(sub.endsAt)}</span>
      </div>
      <QuotaBar label="周期额度" used={sub.usedMicros} total={sub.quotaMicros} currency={currency} testId={`account-sub-quota-${sub.id}`} />
      {sub.dailyLimitMicros > 0 && (
        <QuotaBar label="今日上限" used={sub.dailyUsedMicros} total={sub.dailyLimitMicros} currency={currency} testId={`account-sub-daily-${sub.id}`} />
      )}
    </div>
  );
}

function redeemText(r: RedeemResult, currency: string): string {
  if (r.kind === 'plan') {
    const name = r.plan?.name ?? r.subscription?.planName ?? '套餐';
    return `兑换成功:已开通「${name}」,当前余额 ${formatMoney(r.balanceMicros, currency)}`;
  }
  return `兑换成功:余额 +${formatMoney(r.valueMicros, currency)},当前余额 ${formatMoney(r.balanceMicros, currency)}`;
}

function BalanceSection({ status }: { status: AccountStatus }) {
  const refreshStatus = useAccountStore((st) => st.refreshStatus);
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<RedeemResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const redeem = async () => {
    const c = code.trim();
    if (c === '' || busy) return;
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const r = await postRedeem(c);
      setResult(r);
      setCode('');
      await refreshStatus();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const active = status.subscriptions.filter((s) => s.status === 'active');
  const shown = active.length > 0 ? active : status.subscriptions;

  return (
    <SetCard testId="account-balance-card">
      <div className="flex items-end justify-between gap-4 border-b border-edge px-5 py-4">
        <div className="flex shrink-0 flex-col gap-2">
          <span className="text-[12px] text-fg-3">可用余额</span>
          <span
            data-testid="account-balance"
            className="whitespace-nowrap font-serif text-[30px] font-semibold leading-none tracking-tight text-fg tabular-nums"
          >
            {formatAmount(status.balanceMicros)}{' '}
            <span className="font-sans text-[13px] font-medium tracking-normal text-fg-3">{status.currency || 'USD'}</span>
          </span>
        </div>
        <span className="max-w-[220px] text-right text-[11.5px] leading-[17px] text-fg-3">
          套餐额度优先扣除,不足部分扣余额
        </span>
      </div>
      {shown.length === 0 ? (
        <SetRow title="套餐" desc="暂无生效套餐,可用兑换码开通" testId="account-sub-empty" />
      ) : (
        shown.map((sub) => <SubscriptionRow key={sub.id} sub={sub} currency={status.currency} />)
      )}
      <div className="flex flex-col gap-2 px-4 py-3.5">
        <div className="text-[13px] font-medium text-fg">兑换码</div>
        <div className="flex items-center gap-1.5">
          <SetInput value={code} onChange={setCode} width={260} testId="account-redeem-input" placeholder="输入兑换码" />
          <SmBtn accent label={busy ? '兑换中…' : '兑换'} testId="account-redeem-submit" disabled={busy || code.trim() === ''} onClick={() => void redeem()} />
        </div>
        {result && (
          <span data-testid="account-redeem-result" className="text-[11.5px] text-sage">
            {redeemText(result, status.currency)}
          </span>
        )}
        {error && (
          <span data-testid="account-redeem-error" className="text-[11.5px] text-danger">
            {error}
          </span>
        )}
      </div>
    </SetCard>
  );
}


// ---------- 页面 ----------

export default function AccountPage() {
  const status = useAccountStore((st) => st.status);
  const loading = useAccountStore((st) => st.loading);
  const storeError = useAccountStore((st) => st.error);
  const refreshStatus = useAccountStore((st) => st.refreshStatus);
  const openAuth = useAccountStore((st) => st.openAuth);
  const logout = useAccountStore((st) => st.logout);
  const [loggingOut, setLoggingOut] = useState(false);
  const [usageRange] = useState(recentUsageRange);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const user = status?.loggedIn ? status.user : null;

  if (!status || !user) {
    return (
      <div data-testid="settings-page-account" className="flex flex-col">
        <SetH1>账户</SetH1>
        {!status && !loading && storeError ? (
          <SetCard testId="account-status-error">
            <SetRow
              title="无法读取账户状态"
              desc={storeError}
              last
              control={<SmBtn label="重试" testId="account-status-retry" onClick={() => void refreshStatus()} />}
            />
          </SetCard>
        ) : (
          <SetCard testId="account-logged-out">
            <div className="flex flex-col items-center gap-3 px-6 py-10 text-center">
              <ForgeMark variant="folded" size={44} className="mb-1 text-fg" />
              <div className="font-serif text-[20px] font-semibold text-fg">登录 RurixForge 云</div>
              <div className="max-w-[400px] text-balance text-[12.5px] leading-[19px] text-fg-2">
                登录后可使用云端模型、查看余额与用量,设置与记忆随账号同步。
              </div>
              <button
                type="button"
                data-testid="account-login"
                onClick={openAuth}
                className="mt-2 flex h-9 items-center rounded-xl bg-fg px-5 text-[13px] font-medium text-shell-bg transition-opacity hover:opacity-90"
              >
                登录或注册
              </button>
            </div>
          </SetCard>
        )}
        {status?.serverUrl && (
          <div className="mt-2 pl-0.5 text-[11px] text-fg-4">
            服务器 <span className="font-code">{status.serverUrl}</span>
            {status.reachable ? '' : ' · 暂不可达'}
          </div>
        )}
      </div>
    );
  }

  return (
    <div data-testid="settings-page-account" className="flex flex-col">
      <SetH1>账户</SetH1>
      {!status.reachable && (
        <div data-testid="account-unreachable" className="mb-3 rounded-lg bg-warn-bg px-3 py-2 text-[11.5px] text-warn">
          暂时连不上 RurixForge 云({status.serverUrl}),以下为缓存信息。
        </div>
      )}
      {status.lastError && (
        <div className="mb-3 rounded-lg bg-warn-bg px-3 py-2 text-[11.5px] text-warn">{status.lastError}</div>
      )}
      <div className="account-columns">
        <div data-testid="account-details" className="account-overview">
          <ProfileSection key={user.id} user={user} />
          <BalanceSection status={status} />
        </div>
        <aside data-testid="account-insights" className="min-w-0">
          <UsageDashboard key={`usage-${user.id}`} currency={status.currency} {...usageRange} chart details={false} />
        </aside>
      </div>
      <div className="mt-5 flex flex-wrap items-center gap-2.5">
        <SmBtn
          label={<><LogOut size={11} />{loggingOut ? '退出中…' : '退出登录'}</>}
          testId="account-logout"
          disabled={loggingOut}
          onClick={() => {
            setLoggingOut(true);
            void logout().finally(() => setLoggingOut(false));
          }}
        />
        <span className="text-[10.5px] text-fg-4">退出后清除本机登录信息</span>
      </div>
    </div>
  );
}
