import { Pencil, Plus, RefreshCw, Star, Trash2 } from 'lucide-react';
import { useId, useState, type FormEvent } from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { useConfirm } from '@/components/ui/confirm';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Checkbox, Field, Input, Select, Textarea } from '@/components/ui/form';
import { Card, CardHeader, FormError, PageHeader } from '@/components/ui/misc';
import { Table, TableStatus, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { Group, Plan } from '@/lib/api/types';
import { unitsToMicros } from '@/lib/format';
import {
  FEATURES_MAX,
  FREE_TIER,
  formatZhe,
  planFromForm,
  planToForm,
  splitPlans,
  TAGLINE_MAX,
  yearlyDiscount,
  type PlanForm,
  type PlanKind,
} from '@/lib/plans';
import { groupNameMap, useGroups, usePlans } from '@/lib/queries';
import { useMoney, useSettings } from '@/lib/settings';
import { useToast } from '@/lib/toast';

type Editing = { plan: Plan | null; kind: PlanKind } | null;

export function PlansPage() {
  const plans = usePlans();
  const groups = useGroups();
  const money = useMoney();
  const toast = useToast();
  const [confirmEl, confirm] = useConfirm();
  const [editing, setEditing] = useState<Editing>(null);
  const groupNames = groupNameMap(groups.data);
  const { tiers, packs } = splitPlans(plans.data ?? []);

  const onDelete = (p: Plan) =>
    confirm({
      title: `删除${p.tier ? '档位' : '套餐'}「${p.name}」`,
      description: '已有订阅记录的套餐不能删除（会提示改为停用）；如仅想下架，请改为停用。',
      confirmText: '删除',
      danger: true,
      action: async () => {
        await adminApi.plans.remove(p.id);
        toast.success('已删除');
        plans.reload();
      },
    });

  const rowActions = (p: Plan) => (
    <div className="flex justify-end gap-1">
      <Button size="icon-sm" variant="ghost" aria-label={`编辑 ${p.name}`} onClick={() => setEditing({ plan: p, kind: p.tier ? 'tier' : 'pack' })}>
        <Pencil />
      </Button>
      <Button size="icon-sm" variant="danger-ghost" aria-label={`删除 ${p.name}`} onClick={() => onDelete(p)}>
        <Trash2 />
      </Button>
    </div>
  );
  const statusBadge = (p: Plan) => (p.enabled ? <Badge tone="success">启用</Badge> : <Badge tone="neutral">停用</Badge>);
  const quota = (v: number) => (v > 0 ? money(v) : '—');

  return (
    <div>
      <PageHeader
        title="套餐"
        description="会员档位参照 Cursor 个人方案：额度按月重置，可按月/按年购买，升级即时生效并按剩余价值抵扣，降级下个周期生效。额度包按整段周期提供额度，通过兑换码或后台开通。"
        actions={
          <>
            <Button size="sm" variant="ghost" onClick={plans.reload}>
              <RefreshCw />
              刷新
            </Button>
            <Button size="sm" onClick={() => setEditing({ plan: null, kind: 'pack' })}>
              <Plus />
              新建额度包
            </Button>
            <Button size="sm" variant="primary" onClick={() => setEditing({ plan: null, kind: 'tier' })}>
              <Plus />
              新建档位
            </Button>
          </>
        }
      />

      <Card className="mb-4">
        <CardHeader title="会员档位" />
        <Table>
          <THead>
            <tr>
              <TH>档位</TH>
              <TH className="text-right">级别</TH>
              <TH className="text-right">月付</TH>
              <TH className="text-right">年付</TH>
              <TH className="text-right">第三方 API 池 / 月</TH>
              <TH className="text-right">平台模型池 / 月</TH>
              <TH className="text-right">每日上限</TH>
              <TH className="text-right">生效订阅</TH>
              <TH>状态</TH>
              <TH className="text-right">操作</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus
              colSpan={10}
              loading={plans.loading}
              error={plans.error}
              empty={tiers.length === 0}
              emptyText="暂无会员档位"
              onRetry={plans.reload}
            />
            {tiers.map((p) => {
              const disc = yearlyDiscount(p.priceMicros, p.priceYearlyMicros);
              const free = p.tier === FREE_TIER;
              return (
                <TR key={p.id}>
                  <TD>
                    <div className="flex items-center gap-1.5 font-medium">
                      {p.name}
                      <Badge tone="outline" className="font-mono">
                        {p.tier}
                      </Badge>
                      {p.highlight ? (
                        <Badge tone="info">
                          <Star />
                          推荐
                        </Badge>
                      ) : null}
                    </div>
                    <div className="max-w-72 truncate text-[11px] text-muted-foreground" title={p.tagline || p.description}>
                      {free ? '免费档：其额度即无档位订阅用户每月（UTC 自然月）的免费额度' : p.tagline || p.description || `ID ${p.id}`}
                    </div>
                  </TD>
                  <TD className="tabular text-right">{p.tierRank}</TD>
                  <TD className="tabular whitespace-nowrap text-right">{p.priceMicros > 0 ? money(p.priceMicros) : '免费'}</TD>
                  <TD className="tabular whitespace-nowrap text-right">
                    {p.priceYearlyMicros > 0 ? (
                      <>
                        {money(p.priceYearlyMicros)}
                        {disc && formatZhe(disc.percent) ? <div className="text-[11px] text-success">{formatZhe(disc.percent)}</div> : null}
                      </>
                    ) : (
                      '—'
                    )}
                  </TD>
                  <TD className="tabular whitespace-nowrap text-right">{quota(p.quotaMicros)}</TD>
                  <TD className="tabular whitespace-nowrap text-right">{quota(p.forgeQuotaMicros)}</TD>
                  <TD className="tabular whitespace-nowrap text-right">{p.dailyLimitMicros > 0 ? money(p.dailyLimitMicros) : '不限'}</TD>
                  <TD className="tabular text-right">{p.subscriberCount}</TD>
                  <TD>{statusBadge(p)}</TD>
                  <TD>{rowActions(p)}</TD>
                </TR>
              );
            })}
          </TBody>
        </Table>
      </Card>

      <Card>
        <CardHeader title="额度包" />
        <Table>
          <THead>
            <tr>
              <TH>名称</TH>
              <TH>描述</TH>
              <TH className="text-right">价格</TH>
              <TH className="text-right">周期</TH>
              <TH className="text-right">第三方 API 池</TH>
              <TH className="text-right">平台模型池</TH>
              <TH className="text-right">每日上限</TH>
              <TH>分组</TH>
              <TH className="text-right">生效订阅</TH>
              <TH>状态</TH>
              <TH className="text-right">操作</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus
              colSpan={11}
              loading={plans.loading}
              error={plans.error}
              empty={packs.length === 0}
              emptyText="暂无额度包"
              onRetry={plans.reload}
            />
            {packs.map((p) => (
              <TR key={p.id}>
                <TD>
                  <div className="font-medium">{p.name}</div>
                  <div className="text-[11px] text-muted-foreground">ID {p.id}</div>
                </TD>
                <TD className="max-w-56 truncate text-xs text-muted-foreground" title={p.description}>
                  {p.description || '—'}
                </TD>
                <TD className="tabular whitespace-nowrap text-right">{money(p.priceMicros)}</TD>
                <TD className="tabular whitespace-nowrap text-right">{p.periodDays} 天</TD>
                <TD className="tabular whitespace-nowrap text-right">{quota(p.quotaMicros)}</TD>
                <TD className="tabular whitespace-nowrap text-right">{quota(p.forgeQuotaMicros)}</TD>
                <TD className="tabular whitespace-nowrap text-right">{p.dailyLimitMicros > 0 ? money(p.dailyLimitMicros) : '不限'}</TD>
                <TD className="text-xs">{p.groupId == null ? '—' : groupNames.get(p.groupId) ?? `#${p.groupId}`}</TD>
                <TD className="tabular text-right">{p.subscriberCount}</TD>
                <TD>{statusBadge(p)}</TD>
                <TD>{rowActions(p)}</TD>
              </TR>
            ))}
          </TBody>
        </Table>
      </Card>

      <Dialog open={editing !== null} onOpenChange={(o) => !o && setEditing(null)}>
        {editing !== null ? (
          <PlanDialog
            plan={editing.plan}
            kind={editing.kind}
            groups={groups.data ?? []}
            onClose={() => setEditing(null)}
            onSaved={plans.reload}
          />
        ) : null}
      </Dialog>
      {confirmEl}
    </div>
  );
}

export function PlanDialog({
  plan,
  kind,
  groups,
  onClose,
  onSaved,
}: {
  plan: Plan | null;
  kind: PlanKind;
  groups: Group[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const formId = useId();
  const toast = useToast();
  const money = useMoney();
  const { currency } = useSettings();
  const [form, setForm] = useState<PlanForm>(() => planToForm(plan, kind));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<PlanForm>) => setForm((f) => ({ ...f, ...patch }));
  const isTier = form.kind === 'tier';
  const disc = isTier ? yearlyDiscount(unitsToMicros(form.price || '0'), unitsToMicros(form.priceYearly || '0')) : null;
  const noun = isTier ? '档位' : '额度包';

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const parsed = planFromForm(form);
    if (!parsed.ok) return setError(parsed.error);
    setError(null);
    setBusy(true);
    try {
      if (plan) await adminApi.plans.update(plan.id, parsed.value);
      else await adminApi.plans.create(parsed.value);
      toast.success(plan ? `${noun}已保存` : `${noun}已创建`);
      onSaved();
      onClose();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogContent
      title={plan ? `编辑${noun}「${plan.name}」` : `新建${noun}`}
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button type="submit" form={formId} variant="primary" loading={busy}>
            保存
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={onSubmit} className="grid grid-cols-2 gap-3" noValidate>
        <Field label="名称" required>
          <Input value={form.name} onChange={(e) => set({ name: e.target.value })} autoFocus />
        </Field>
        {isTier ? (
          <Field label="档位标识" required hint="小写字母开头，如 pro、pro_plus；全局唯一">
            <Input value={form.tier} onChange={(e) => set({ tier: e.target.value })} className="font-mono" />
          </Field>
        ) : (
          <Field label={`价格（${currency}）`} hint="展示用；兑换码/后台开通不扣费">
            <Input inputMode="decimal" value={form.price} onChange={(e) => set({ price: e.target.value })} placeholder="0" />
          </Field>
        )}
        {isTier ? (
          <>
            <Field label="档位级别" hint="0–1000，越大越高；决定升级/降级">
              <Input inputMode="numeric" value={form.tierRank} onChange={(e) => set({ tierRank: e.target.value })} />
            </Field>
            <Field label="标语" hint={`价格页副标题，≤ ${TAGLINE_MAX} 字`}>
              <Input value={form.tagline} onChange={(e) => set({ tagline: e.target.value })} />
            </Field>
            <Field label={`月付价格（${currency}）`} hint="0 = 不支持按月购买">
              <Input inputMode="decimal" value={form.price} onChange={(e) => set({ price: e.target.value })} placeholder="0" />
            </Field>
            <Field
              label={`年付价格（${currency}）`}
              hint={
                disc
                  ? `折合每月 ${money(disc.perMonthMicros)}${formatZhe(disc.percent) ? `，${formatZhe(disc.percent)}（比 12 × 月付省 ${money(disc.savedMicros)}）` : ''}`
                  : '留空或 0 = 不支持按年购买'
              }
            >
              <Input inputMode="decimal" value={form.priceYearly} onChange={(e) => set({ priceYearly: e.target.value })} placeholder="0" />
            </Field>
          </>
        ) : null}
        <Field label="描述" className="col-span-2">
          <Textarea value={form.description} onChange={(e) => set({ description: e.target.value })} className="min-h-14" />
        </Field>
        <Field label={`第三方 API 池额度（${currency}）`} hint={isTier ? '每月重置；按 API 价计入' : '整个周期'}>
          <Input inputMode="decimal" value={form.quota} onChange={(e) => set({ quota: e.target.value })} placeholder="0" />
        </Field>
        <Field label={`平台模型池额度（${currency}）`} hint={isTier ? '每月重置' : '整个周期'}>
          <Input inputMode="decimal" value={form.forgeQuota} onChange={(e) => set({ forgeQuota: e.target.value })} placeholder="0" />
        </Field>
        <Field label={`每日上限（${currency}）`} hint="0 = 不限；两池合计">
          <Input inputMode="decimal" value={form.dailyLimit} onChange={(e) => set({ dailyLimit: e.target.value })} />
        </Field>
        <Field label={isTier ? '开通天数' : '周期（天）'} required hint={isTier ? '后台开通/兑换码的默认天数；购买按自然月/年' : undefined}>
          <Input inputMode="numeric" value={form.periodDays} onChange={(e) => set({ periodDays: e.target.value })} />
        </Field>
        <Field label="绑定分组" hint="订阅生效期间按此分组计费与调度；档位一般不绑定" className="col-span-2">
          <Select value={form.groupId} onChange={(e) => set({ groupId: e.target.value })}>
            <option value="">不绑定</option>
            {groups.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </Select>
        </Field>
        {isTier ? (
          <Field label="权益" hint={`价格页卡片上的权益列表，一行一条，最多 ${FEATURES_MAX} 条`} className="col-span-2">
            <Textarea value={form.features} onChange={(e) => set({ features: e.target.value })} className="min-h-24" />
          </Field>
        ) : null}
        <div className="col-span-2 flex flex-wrap gap-x-6 gap-y-2">
          <Checkbox label="启用" hint="停用后不再出现在价格页与公开套餐列表" checked={form.enabled} onChange={(e) => set({ enabled: e.target.checked })} />
          {isTier ? (
            <Checkbox label="推荐" hint="价格页高亮此档位" checked={form.highlight} onChange={(e) => set({ highlight: e.target.checked })} />
          ) : null}
        </div>
        <div className="col-span-2">
          <FormError>{error}</FormError>
        </div>
      </form>
    </DialogContent>
  );
}
