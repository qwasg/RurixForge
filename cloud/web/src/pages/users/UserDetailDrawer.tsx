import { Eye, EyeOff } from 'lucide-react';
import { useState, type FormEvent } from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { useConfirm } from '@/components/ui/confirm';
import { Dialog, DrawerContent } from '@/components/ui/dialog';
import { Field, Input, Select } from '@/components/ui/form';
import { Card, CardHeader, ErrorBlock, FormError, LoadingBlock } from '@/components/ui/misc';
import { Table, TableStatus, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { AdminUser, ApiKey, Device, Group, LedgerEntry, Plan, Role, Subscription, UserStatus } from '@/lib/api/types';
import { INTERVAL_LABEL, SUBSCRIPTION_SOURCE_LABEL } from '@/lib/plans';
import { useAuth } from '@/lib/auth';
import { formatDate, formatDateTime, formatNumber, parseIntInput, unitsToMicros } from '@/lib/format';
import { useAsync } from '@/lib/hooks';
import { usePlans } from '@/lib/queries';
import { useMoney, useSettings } from '@/lib/settings';
import { useToast } from '@/lib/toast';
import { cn } from '@/lib/utils';
import { ledgerKindLabel, ROLE_LABEL, ROLE_TONE, USER_STATUS_LABEL, USER_STATUS_TONE } from './labels';

interface Props {
  userId: number | null;
  onOpenChange: (open: boolean) => void;
  groups: Group[];
  /** 用户资料/余额变化后通知列表刷新。 */
  onChanged: () => void;
}

export function UserDetailDrawer({ userId, onOpenChange, groups, onChanged }: Props) {
  return (
    <Dialog open={userId !== null} onOpenChange={onOpenChange}>
      {userId !== null ? <UserDetailBody userId={userId} groups={groups} onChanged={onChanged} /> : null}
    </Dialog>
  );
}

function UserDetailBody({ userId, groups, onChanged }: { userId: number; groups: Group[]; onChanged: () => void }) {
  const detail = useAsync(() => adminApi.users.get(userId), [userId]);
  const plans = usePlans();
  const { user: me } = useAuth();
  const u = detail.data?.user;
  const refresh = () => {
    detail.reload();
    onChanged();
  };

  return (
    <DrawerContent
      title={u ? u.email : '用户详情'}
      description={u ? `ID ${u.id} · 注册于 ${formatDateTime(u.createdAt)} · 最近登录 ${formatDateTime(u.lastLoginAt)}` : undefined}
    >
      {!detail.data || !u ? (
        detail.error ? (
          <ErrorBlock error={detail.error} onRetry={detail.reload} />
        ) : (
          <LoadingBlock />
        )
      ) : (
        <>
          <div className="mb-3 flex flex-wrap items-center gap-2">
            <Badge tone={ROLE_TONE[u.role]}>{ROLE_LABEL[u.role]}</Badge>
            <Badge tone={USER_STATUS_TONE[u.status]}>{USER_STATUS_LABEL[u.status]}</Badge>
            {u.groupName ? <Badge tone="outline">分组：{u.groupName}</Badge> : null}
          </div>
          <Tabs defaultValue="profile">
            <TabsList>
              <TabsTrigger value="profile">资料</TabsTrigger>
              <TabsTrigger value="balance">余额与流水</TabsTrigger>
              <TabsTrigger value="subs">订阅（{detail.data.subscriptions?.length ?? 0}）</TabsTrigger>
              <TabsTrigger value="devices">登录设备（{detail.data.devices?.length ?? 0}）</TabsTrigger>
              <TabsTrigger value="keys">API Key（{detail.data.apiKeys?.length ?? 0}）</TabsTrigger>
            </TabsList>
            <TabsContent value="profile">
              <div className="flex flex-col gap-4">
                <ProfileCard user={u} groups={groups} isSelf={me?.id === u.id} onSaved={refresh} />
                <PasswordCard userId={u.id} />
              </div>
            </TabsContent>
            <TabsContent value="balance">
              <BalanceCard user={u} ledger={detail.data.ledger ?? []} onChanged={refresh} />
            </TabsContent>
            <TabsContent value="subs">
              <SubscriptionsCard userId={u.id} subs={detail.data.subscriptions ?? []} plans={plans.data ?? []} onChanged={refresh} />
            </TabsContent>
            <TabsContent value="devices">
              <DevicesCard devices={detail.data.devices ?? []} />
            </TabsContent>
            <TabsContent value="keys">
              <ApiKeysCard keys={detail.data.apiKeys ?? []} />
            </TabsContent>
          </Tabs>
        </>
      )}
    </DrawerContent>
  );
}

function ProfileCard({ user, groups, isSelf, onSaved }: { user: AdminUser; groups: Group[]; isSelf: boolean; onSaved: () => void }) {
  const toast = useToast();
  const [nickname, setNickname] = useState(user.nickname);
  const [role, setRole] = useState<Role>(user.role);
  const [status, setStatus] = useState<UserStatus>(user.status);
  const [groupId, setGroupId] = useState(user.groupId == null ? '' : String(user.groupId));
  const [override, setOverride] = useState(user.concurrencyOverride == null ? '' : String(user.concurrencyOverride));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const concurrencyOverride = override.trim() === '' ? null : parseIntInput(override);
    if (concurrencyOverride !== null && (Number.isNaN(concurrencyOverride) || concurrencyOverride < 0)) {
      return setError('并发覆盖应为非负整数，留空表示跟随分组');
    }
    setError(null);
    setBusy(true);
    try {
      await adminApi.users.update(user.id, {
        nickname: nickname.trim(),
        ...(isSelf ? {} : { role, status }),
        groupId: groupId ? Number(groupId) : null,
        concurrencyOverride,
      });
      toast.success('资料已保存');
      onSaved();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card>
      <CardHeader title="基本资料" />
      <form onSubmit={onSubmit} className="grid grid-cols-2 gap-3 p-4" noValidate>
        <Field label="昵称" className="col-span-2">
          <Input value={nickname} onChange={(e) => setNickname(e.target.value)} maxLength={32} />
        </Field>
        <Field label="角色" hint={isSelf ? '不能修改自己的角色' : undefined}>
          <Select value={role} onChange={(e) => setRole(e.target.value as Role)} disabled={isSelf}>
            <option value="user">用户</option>
            <option value="admin">管理员</option>
          </Select>
        </Field>
        <Field label="状态" hint={isSelf ? '不能禁用自己' : '禁用后该用户所有请求被拒绝'}>
          <Select value={status} onChange={(e) => setStatus(e.target.value as UserStatus)} disabled={isSelf}>
            <option value="active">正常</option>
            <option value="disabled">已禁用</option>
          </Select>
        </Field>
        <Field label="分组">
          <Select value={groupId} onChange={(e) => setGroupId(e.target.value)}>
            <option value="">默认分组</option>
            {groups.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="并发覆盖" hint="留空 = 使用分组的每用户并发">
          <Input inputMode="numeric" value={override} onChange={(e) => setOverride(e.target.value)} placeholder="跟随分组" />
        </Field>
        <div className="col-span-2 flex items-center justify-between gap-3">
          <FormError>{error}</FormError>
          <Button type="submit" variant="primary" loading={busy} className="ml-auto">
            保存资料
          </Button>
        </div>
      </form>
    </Card>
  );
}

function PasswordCard({ userId }: { userId: number }) {
  const toast = useToast();
  const [password, setPassword] = useState('');
  const [show, setShow] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (password.length < 8) return setError('新密码至少 8 位');
    setError(null);
    setBusy(true);
    try {
      await adminApi.users.setPassword(userId, password);
      setPassword('');
      toast.success('密码已重置');
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card>
      <CardHeader title="重置密码" />
      <form onSubmit={onSubmit} className="flex flex-col gap-2 p-4" noValidate>
        <div className="flex items-end gap-2">
          <Field label="新密码" className="flex-1" hint="至少 8 位">
            <Input
              type={show ? 'text' : 'password'}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="new-password"
            />
          </Field>
          <Button variant="ghost" size="icon" className="mb-5" aria-label={show ? '隐藏密码' : '显示密码'} onClick={() => setShow((s) => !s)}>
            {show ? <EyeOff /> : <Eye />}
          </Button>
          <Button type="submit" loading={busy} className="mb-5">
            设置密码
          </Button>
        </div>
        <FormError>{error}</FormError>
      </form>
    </Card>
  );
}

function BalanceCard({ user, ledger, onChanged }: { user: AdminUser; ledger: LedgerEntry[]; onChanged: () => void }) {
  const toast = useToast();
  const money = useMoney();
  const { currency } = useSettings();
  const [amount, setAmount] = useState('');
  const [note, setNote] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const delta = amount.trim() === '' ? Number.NaN : unitsToMicros(amount);

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (Number.isNaN(delta) || delta === 0) return setError('请输入非零金额（负数为扣减）');
    setError(null);
    setBusy(true);
    try {
      const res = await adminApi.users.adjustBalance(user.id, delta, note.trim() || '管理员调整');
      setAmount('');
      setNote('');
      toast.success(`余额已更新为 ${money(res?.balanceMicros ?? user.balanceMicros + delta)}`);
      onChanged();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader title="调整余额" />
        <div className="p-4">
          <div className="mb-3 flex items-baseline gap-2">
            <span className="text-xs text-muted-foreground">当前余额</span>
            <span className={cn('tabular text-xl font-semibold', user.balanceMicros < 0 && 'text-danger')}>{money(user.balanceMicros)}</span>
          </div>
          <form onSubmit={onSubmit} className="flex flex-col gap-2" noValidate>
            <div className="grid grid-cols-[10rem_1fr_auto] items-start gap-2">
              <Field label={`金额（${currency}）`} hint={!Number.isNaN(delta) && delta !== 0 ? `调整后 ${money(user.balanceMicros + delta)}` : '负数为扣减'}>
                <Input inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="如 10 或 -5.5" />
              </Field>
              <Field label="备注">
                <Input value={note} onChange={(e) => setNote(e.target.value)} placeholder="管理员调整" maxLength={200} />
              </Field>
              <Button type="submit" variant="primary" loading={busy} className="mt-5">
                确认调整
              </Button>
            </div>
            <FormError>{error}</FormError>
          </form>
        </div>
      </Card>
      <Card>
        <CardHeader title="最近流水（20 条）" />
        <Table>
          <THead>
            <tr>
              <TH>时间</TH>
              <TH>类型</TH>
              <TH className="text-right">变动</TH>
              <TH className="text-right">变动后余额</TH>
              <TH>备注</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus colSpan={5} loading={false} error={null} empty={ledger.length === 0} emptyText="暂无流水" />
            {ledger.map((l) => (
              <TR key={l.id}>
                <TD className="whitespace-nowrap text-xs">{formatDateTime(l.createdAt)}</TD>
                <TD>
                  <Badge tone="outline">{ledgerKindLabel(l.kind)}</Badge>
                </TD>
                <TD className={cn('tabular text-right', l.deltaMicros < 0 ? 'text-danger' : 'text-success')}>
                  {l.deltaMicros > 0 ? '+' : ''}
                  {money(l.deltaMicros)}
                </TD>
                <TD className="tabular text-right">{money(l.balanceAfterMicros)}</TD>
                <TD className="max-w-60 truncate text-xs text-muted-foreground" title={l.note}>
                  {l.note || '—'}
                </TD>
              </TR>
            ))}
          </TBody>
        </Table>
      </Card>
    </div>
  );
}

const SUB_STATUS: Record<Subscription['status'], { label: string; tone: 'success' | 'neutral' | 'danger' }> = {
  active: { label: '生效中', tone: 'success' },
  expired: { label: '已过期', tone: 'neutral' },
  cancelled: { label: '已取消', tone: 'danger' },
};

function SubscriptionsCard({
  userId,
  subs,
  plans,
  onChanged,
}: {
  userId: number;
  subs: Subscription[];
  plans: Plan[];
  onChanged: () => void;
}) {
  const toast = useToast();
  const money = useMoney();
  const [confirmEl, confirm] = useConfirm();
  const [planId, setPlanId] = useState('');
  const [days, setDays] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const sortedPlans = [...plans].sort((a, b) => Number(b.enabled) - Number(a.enabled));

  const onPlanChange = (value: string) => {
    setPlanId(value);
    const plan = plans.find((p) => String(p.id) === value);
    if (plan) setDays(String(plan.periodDays));
  };

  const onGrant = async (e: FormEvent) => {
    e.preventDefault();
    if (!planId) return setError('请选择套餐');
    const n = parseIntInput(days);
    if (Number.isNaN(n) || n <= 0) return setError('天数应为正整数');
    setError(null);
    setBusy(true);
    try {
      await adminApi.users.grantSubscription(userId, Number(planId), n);
      toast.success('已开通订阅');
      setPlanId('');
      setDays('');
      onChanged();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader title="开通订阅" />
        <form onSubmit={onGrant} className="flex flex-col gap-2 p-4" noValidate>
          <div className="grid grid-cols-[1fr_8rem_auto] items-start gap-2">
            <Field label="套餐">
              <Select value={planId} onChange={(e) => onPlanChange(e.target.value)}>
                <option value="">选择套餐…</option>
                {sortedPlans.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                    {p.enabled ? '' : '（已停用）'} · {money(p.quotaMicros)} / {p.periodDays} 天
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="天数">
              <Input inputMode="numeric" value={days} onChange={(e) => setDays(e.target.value)} placeholder="套餐周期" />
            </Field>
            <Button type="submit" variant="primary" loading={busy} className="mt-5">
              开通
            </Button>
          </div>
          <FormError>{error}</FormError>
        </form>
      </Card>
      <Card>
        <CardHeader title="订阅记录" />
        <Table>
          <THead>
            <tr>
              <TH>套餐</TH>
              <TH>状态</TH>
              <TH>有效期</TH>
              <TH className="text-right">第三方 API 池</TH>
              <TH className="text-right">平台模型池</TH>
              <TH>本周期至</TH>
              <TH className="text-right">今日</TH>
              <TH className="text-right">操作</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus colSpan={8} loading={false} error={null} empty={subs.length === 0} emptyText="暂无订阅" />
            {subs.map((s) => {
              const st = SUB_STATUS[s.status] ?? { label: s.status, tone: 'neutral' as const };
              const scheduled = s.status === 'active' && new Date(s.startsAt).getTime() > Date.now();
              return (
                <TR key={s.id}>
                  <TD>
                    <div className="flex items-center gap-1.5">
                      {s.planName || `#${s.planId}`}
                      {s.tier ? (
                        <Badge tone="outline" className="font-mono">
                          {s.tier}
                        </Badge>
                      ) : null}
                    </div>
                    <div className="text-[11px] text-muted-foreground">
                      {[
                        SUBSCRIPTION_SOURCE_LABEL[s.source] ?? s.source,
                        s.billingInterval ? INTERVAL_LABEL[s.billingInterval] : '',
                        s.valueMicros > 0 ? `价值 ${money(s.valueMicros)}` : '',
                      ]
                        .filter(Boolean)
                        .join(' · ')}
                    </div>
                  </TD>
                  <TD>{scheduled ? <Badge tone="info">已预约</Badge> : <Badge tone={st.tone}>{st.label}</Badge>}</TD>
                  <TD className="whitespace-nowrap text-xs">
                    {formatDate(s.startsAt)} ~ {formatDate(s.endsAt)}
                  </TD>
                  <TD className="tabular whitespace-nowrap text-right text-xs">
                    {s.quotaMicros > 0 || s.usedMicros > 0 ? `${money(s.usedMicros)} / ${money(s.quotaMicros)}` : '—'}
                  </TD>
                  <TD className="tabular whitespace-nowrap text-right text-xs">
                    {s.forgeQuotaMicros > 0 || s.forgeUsedMicros > 0 ? `${money(s.forgeUsedMicros)} / ${money(s.forgeQuotaMicros)}` : '—'}
                  </TD>
                  <TD className="whitespace-nowrap text-xs" title={s.usageCycle === 'month' ? '额度按月重置' : '整段有效期一个周期'}>
                    {s.cycleEnd ? formatDate(s.cycleEnd) : '—'}
                  </TD>
                  <TD className="tabular whitespace-nowrap text-right text-xs">
                    {s.dailyLimitMicros > 0 ? `${money(s.dailyUsedMicros)} / ${money(s.dailyLimitMicros)}` : '不限'}
                  </TD>
                  <TD className="text-right">
                    {s.status === 'active' ? (
                      <Button
                        size="sm"
                        variant="danger-ghost"
                        onClick={() =>
                          confirm({
                            title: '取消订阅',
                            description: `确定取消「${s.planName || s.planId}」订阅？剩余额度将立即失效。`,
                            confirmText: '取消订阅',
                            danger: true,
                            action: async () => {
                              await adminApi.users.cancelSubscription(userId, s.id);
                              toast.success('订阅已取消');
                              onChanged();
                            },
                          })
                        }
                      >
                        取消
                      </Button>
                    ) : null}
                  </TD>
                </TR>
              );
            })}
          </TBody>
        </Table>
      </Card>
      {confirmEl}
    </div>
  );
}

function DevicesCard({ devices }: { devices: Device[] }) {
  return (
    <Card>
      <Table>
        <THead>
          <tr>
            <TH>设备</TH>
            <TH>平台 / 版本</TH>
            <TH>IP</TH>
            <TH>首次登录</TH>
            <TH>最近活跃</TH>
            <TH>状态</TH>
          </tr>
        </THead>
        <TBody>
          <TableStatus colSpan={6} loading={false} error={null} empty={devices.length === 0} emptyText="暂无登录设备" />
          {devices.map((d) => (
            <TR key={d.id}>
              <TD>
                <div className="font-medium">{d.deviceName || '—'}</div>
                <div className="font-mono text-[11px] text-muted-foreground">{d.deviceId}</div>
              </TD>
              <TD className="text-xs">
                {d.platform || '—'} · {d.appVersion || '—'}
              </TD>
              <TD className="font-mono text-xs">{d.ip || '—'}</TD>
              <TD className="whitespace-nowrap text-xs">{formatDateTime(d.createdAt)}</TD>
              <TD className="whitespace-nowrap text-xs">{formatDateTime(d.lastSeenAt)}</TD>
              <TD>{d.revokedAt ? <Badge tone="neutral">已吊销</Badge> : <Badge tone="success">有效</Badge>}</TD>
            </TR>
          ))}
        </TBody>
      </Table>
    </Card>
  );
}

function ApiKeysCard({ keys }: { keys: ApiKey[] }) {
  const money = useMoney();
  return (
    <Card>
      <Table>
        <THead>
          <tr>
            <TH>名称</TH>
            <TH>类型</TH>
            <TH>前缀</TH>
            <TH>状态</TH>
            <TH className="text-right">已用 / 额度</TH>
            <TH>最近使用</TH>
            <TH>过期</TH>
          </tr>
        </THead>
        <TBody>
          <TableStatus colSpan={7} loading={false} error={null} empty={keys.length === 0} emptyText="暂无 API Key" />
          {keys.map((k) => (
            <TR key={k.id}>
              <TD>
                <div>{k.name || '—'}</div>
                {k.deviceName ? <div className="text-xs text-muted-foreground">{k.deviceName}</div> : null}
              </TD>
              <TD>
                <Badge tone="outline">{k.kind === 'device' ? '设备' : '用户'}</Badge>
              </TD>
              <TD className="font-mono text-xs">{k.prefix}</TD>
              <TD>{k.status === 'active' ? <Badge tone="success">有效</Badge> : <Badge tone="neutral">已吊销</Badge>}</TD>
              <TD className="tabular whitespace-nowrap text-right text-xs">
                {money(k.usedMicros)} / {k.quotaMicros > 0 ? money(k.quotaMicros) : '不限'}
              </TD>
              <TD className="whitespace-nowrap text-xs">{formatDateTime(k.lastUsedAt)}</TD>
              <TD className="whitespace-nowrap text-xs">{k.expiresAt ? formatDateTime(k.expiresAt) : '永不'}</TD>
            </TR>
          ))}
        </TBody>
      </Table>
      <p className="px-3 py-2 text-xs text-muted-foreground">共 {formatNumber(keys.length)} 个</p>
    </Card>
  );
}
