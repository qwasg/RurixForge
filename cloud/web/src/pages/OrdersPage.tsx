import { Ban, CircleCheck, RefreshCw, Search } from 'lucide-react';
import { useId, useState, type FormEvent } from 'react';
import { Badge, type BadgeTone } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { useConfirm } from '@/components/ui/confirm';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Field, Input, Select, Textarea } from '@/components/ui/form';
import { Card, FormError, PageHeader, Toolbar } from '@/components/ui/misc';
import { Pagination, Table, TableStatus, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { AdminOrder, OrderKind, OrderMode, OrderStatus } from '@/lib/api/types';
import { formatDateTime } from '@/lib/format';
import { useAsync, useDebounced, usePagination } from '@/lib/hooks';
import { charCount, INTERVAL_LABEL } from '@/lib/plans';
import { useMoney } from '@/lib/settings';
import { useToast } from '@/lib/toast';

export const ORDER_STATUS_LABEL: Record<OrderStatus, string> = { pending: '待支付', paid: '已支付', cancelled: '已取消', failed: '失败' };
const ORDER_STATUS_TONE: Record<OrderStatus, BadgeTone> = { pending: 'warning', paid: 'success', cancelled: 'neutral', failed: 'danger' };
export const ORDER_KIND_LABEL: Record<OrderKind, string> = { topup: '充值', subscription: '会员' };
export const ORDER_MODE_LABEL: Record<OrderMode, string> = { new: '开通', renew: '续订', upgrade: '升级', downgrade: '降级' };
const NOTE_MAX = 200;

/** 「Pro · 按年 · 升级」；充值单返回「充值」。 */
export function orderSummary(o: Pick<AdminOrder, 'kind' | 'planName' | 'interval' | 'mode'>): string {
  if (o.kind !== 'subscription') return ORDER_KIND_LABEL[o.kind] ?? o.kind;
  const parts = [o.planName || '会员'];
  if (o.interval) parts.push(INTERVAL_LABEL[o.interval]);
  if (o.mode) parts.push(ORDER_MODE_LABEL[o.mode]);
  return parts.join(' · ');
}

export function providerLabel(provider: string): string {
  return provider === 'balance' ? '余额' : provider || '—';
}

export function OrdersPage() {
  const money = useMoney();
  const toast = useToast();
  const [confirmEl, confirm] = useConfirm();
  const [status, setStatus] = useState<OrderStatus | ''>('');
  const [kind, setKind] = useState<OrderKind | ''>('');
  const [q, setQ] = useState('');
  const query = useDebounced(q.trim(), 300);
  const page = usePagination(50);
  const list = useAsync(
    () => adminApi.orders.list({ status, kind, q: query, limit: page.limit, offset: page.offset }),
    [status, kind, query, page.limit, page.offset],
  );
  const [paying, setPaying] = useState<AdminOrder | null>(null);
  const items = list.data?.items ?? [];

  const onCancel = (o: AdminOrder) =>
    confirm({
      title: `取消订单 #${o.id}`,
      description: `${o.userEmail} 的「${orderSummary(o)}」，应付 ${money(o.amountMicros)}。取消后若渠道仍到账，款项会转入用户余额。`,
      confirmText: '取消订单',
      danger: true,
      action: async () => {
        await adminApi.orders.cancel(o.id);
        toast.success('订单已取消');
        list.reload();
      },
    });

  return (
    <div>
      <PageHeader
        title="订单"
        description="会员订单与充值订单。余额支付即时生效；在线支付订单待渠道回调，线下收款后可在此「标记已支付」使其生效。"
      />
      <Card>
        <Toolbar>
          <Select
            aria-label="状态筛选"
            className="w-28"
            value={status}
            onChange={(e) => {
              setStatus(e.target.value as OrderStatus | '');
              page.reset();
            }}
          >
            <option value="">全部状态</option>
            {(Object.keys(ORDER_STATUS_LABEL) as OrderStatus[]).map((s) => (
              <option key={s} value={s}>
                {ORDER_STATUS_LABEL[s]}
              </option>
            ))}
          </Select>
          <Select
            aria-label="类型筛选"
            className="w-28"
            value={kind}
            onChange={(e) => {
              setKind(e.target.value as OrderKind | '');
              page.reset();
            }}
          >
            <option value="">全部类型</option>
            <option value="subscription">会员</option>
            <option value="topup">充值</option>
          </Select>
          <div className="relative w-56">
            <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={q}
              onChange={(e) => {
                setQ(e.target.value);
                page.reset();
              }}
              placeholder="搜索用户邮箱"
              aria-label="搜索用户邮箱"
              className="pl-7"
            />
          </div>
          <Button variant="ghost" size="sm" className="ml-auto" onClick={list.reload} loading={list.loading && !!list.data}>
            {list.loading && list.data ? null : <RefreshCw />}
            刷新
          </Button>
        </Toolbar>
        <Table>
          <THead>
            <tr>
              <TH>订单</TH>
              <TH>用户</TH>
              <TH>内容</TH>
              <TH className="text-right">标价</TH>
              <TH className="text-right">升级抵扣</TH>
              <TH className="text-right">应付</TH>
              <TH>支付方式</TH>
              <TH>状态</TH>
              <TH>支付时间</TH>
              <TH className="text-right">操作</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus
              colSpan={10}
              loading={list.loading}
              error={list.error}
              empty={items.length === 0}
              emptyText="暂无订单"
              onRetry={list.reload}
            />
            {items.map((o) => (
              <TR key={o.id}>
                <TD>
                  <div className="tabular font-medium">#{o.id}</div>
                  <div className="text-[11px] text-muted-foreground">{formatDateTime(o.createdAt)}</div>
                </TD>
                <TD className="max-w-52 truncate text-xs" title={o.userEmail}>
                  {o.userEmail || `#${o.userId}`}
                </TD>
                <TD>
                  <div className="flex items-center gap-1.5">
                    <Badge tone={o.kind === 'subscription' ? 'info' : 'neutral'}>{ORDER_KIND_LABEL[o.kind] ?? o.kind}</Badge>
                    <span className="text-xs">{orderSummary(o)}</span>
                  </div>
                  {o.note ? (
                    <div className="max-w-72 truncate text-[11px] text-muted-foreground" title={o.note}>
                      {o.note}
                    </div>
                  ) : null}
                </TD>
                <TD className="tabular whitespace-nowrap text-right">{o.listPriceMicros > 0 ? money(o.listPriceMicros) : '—'}</TD>
                <TD className="tabular whitespace-nowrap text-right">{o.creditMicros > 0 ? `−${money(o.creditMicros)}` : '—'}</TD>
                <TD className="tabular whitespace-nowrap text-right font-medium">{money(o.amountMicros)}</TD>
                <TD className="text-xs">{providerLabel(o.provider)}</TD>
                <TD>
                  <Badge tone={ORDER_STATUS_TONE[o.status] ?? 'neutral'}>{ORDER_STATUS_LABEL[o.status] ?? o.status}</Badge>
                </TD>
                <TD className="whitespace-nowrap text-xs">{o.paidAt ? formatDateTime(o.paidAt) : '—'}</TD>
                <TD>
                  {o.status === 'pending' ? (
                    <div className="flex justify-end gap-1">
                      <Button size="sm" variant="ghost" onClick={() => setPaying(o)}>
                        <CircleCheck />
                        标记已支付
                      </Button>
                      <Button size="icon-sm" variant="danger-ghost" aria-label={`取消订单 #${o.id}`} onClick={() => onCancel(o)}>
                        <Ban />
                      </Button>
                    </div>
                  ) : null}
                </TD>
              </TR>
            ))}
          </TBody>
        </Table>
        <Pagination
          total={list.data?.total ?? 0}
          limit={page.limit}
          offset={page.offset}
          onOffsetChange={page.setOffset}
          onLimitChange={page.setLimit}
        />
      </Card>

      <Dialog open={paying !== null} onOpenChange={(o) => !o && setPaying(null)}>
        {paying ? <MarkPaidDialog order={paying} onClose={() => setPaying(null)} onDone={list.reload} /> : null}
      </Dialog>
      {confirmEl}
    </div>
  );
}

export function MarkPaidDialog({ order, onClose, onDone }: { order: AdminOrder; onClose: () => void; onDone: () => void }) {
  const formId = useId();
  const money = useMoney();
  const toast = useToast();
  const [note, setNote] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const trimmed = note.trim();
    if (charCount(trimmed) > NOTE_MAX) return setError(`备注最多 ${NOTE_MAX} 字`);
    setError(null);
    setBusy(true);
    try {
      await adminApi.orders.markPaid(order.id, trimmed || undefined);
      toast.success(`订单 #${order.id} 已标记为已支付`);
      onDone();
      onClose();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogContent
      title={`确认收款 · 订单 #${order.id}`}
      description="确认已线下收到款项后，订单立即生效（会员单按升级/续订/降级规则开通），此操作记入审计日志。"
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button type="submit" form={formId} variant="primary" loading={busy}>
            确认已支付
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={onSubmit} className="flex flex-col gap-3" noValidate>
        <div className="rounded-md border bg-muted/40 px-3 py-2 text-xs leading-5">
          <div>用户：{order.userEmail || `#${order.userId}`}</div>
          <div>内容：{orderSummary(order)}</div>
          <div>
            应付：<span className="font-medium">{money(order.amountMicros)}</span>（{providerLabel(order.provider)}）
          </div>
        </div>
        <Field label="备注" hint={`可选，如转账流水号；≤ ${NOTE_MAX} 字`}>
          <Textarea value={note} onChange={(e) => setNote(e.target.value)} className="min-h-16" autoFocus />
        </Field>
        <FormError>{error}</FormError>
      </form>
    </DialogContent>
  );
}
