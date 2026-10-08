import { useState } from 'react';
import { cn } from '@/lib/cn';
import { errorMessage, formatAmount, formatMoney, formatTime, formatTokens } from '@/lib/accountApi';
import {
  cancelOrder,
  orderStatusLabel,
  orderSummary,
  poolLabel,
  providerLabel,
  safePayUrl,
  type MembershipUsage,
  type ModelUsage,
  type Order,
  type OrdersPage,
} from '@/lib/billingApi';
import { SetCard, SmBtn } from '../controls';
import { Badge, formatDay, openPayUrl, toast, type Tone } from './parts';

const numberFmt = new Intl.NumberFormat('en-US');

function UsageCells({ u }: { u: ModelUsage }) {
  return (
    <>
      <td className="px-2 py-2 text-right font-code text-fg-2">{numberFmt.format(u.requests)}</td>
      <td className="px-2 py-2 text-right font-code text-fg-2">
        {formatTokens(u.inputTokens)} / {formatTokens(u.outputTokens)}
      </td>
      <td className="px-2 py-2 text-right font-code text-fg-2">{formatAmount(u.includedMicros)}</td>
      <td className="px-2 py-2 text-right font-code text-fg-2">{formatAmount(u.onDemandMicros)}</td>
      <td className="px-4 py-2 text-right font-code text-fg">{formatAmount(u.costMicros)}</td>
    </>
  );
}

/** 本周期按模型用量(费用倒序):套餐内 / 按量付费拆开显示。 */
export function UsageCard({ usage, currency }: { usage: MembershipUsage; currency: string }) {
  const cur = usage.currency || currency;
  return (
    <SetCard testId="billing-usage">
      <div className="flex items-center gap-2 border-b border-edge px-4 py-2.5 text-[11px] text-fg-3">
        <span className="min-w-0 flex-1">按模型,费用从高到低(金额单位 {cur})</span>
        <span data-testid="billing-usage-range">
          {formatDay(usage.from)} – {formatDay(usage.to)}
        </span>
      </div>
      {usage.items.length === 0 ? (
        <div data-testid="billing-usage-empty" className="px-4 py-3 text-[11.5px] text-fg-4">
          本周期暂无用量
        </div>
      ) : (
        <table data-testid="billing-usage-table" className="w-full table-fixed text-[11.5px]">
          <thead>
            <tr className="text-left text-[10.5px] text-fg-4">
              <th className="w-[32%] px-4 py-2 font-normal">模型</th>
              <th className="px-2 py-2 text-right font-normal">请求</th>
              <th className="px-2 py-2 text-right font-normal">Tokens 入 / 出</th>
              <th className="px-2 py-2 text-right font-normal">套餐内</th>
              <th className="px-2 py-2 text-right font-normal">按量付费</th>
              <th className="px-4 py-2 text-right font-normal">合计</th>
            </tr>
          </thead>
          <tbody>
            {usage.items.map((u) => (
              <tr key={`${u.model}\u0000${u.pool}`} data-testid={`billing-usage-row-${u.model}`} className="border-t border-edge">
                <td className="px-4 py-2">
                  <div className="truncate text-fg" title={u.model}>
                    {u.model}
                  </div>
                  <div className="text-[10.5px] text-fg-4">
                    {poolLabel(u.pool)}
                    {u.errors > 0 ? ` · ${u.errors} 次失败` : ''}
                  </div>
                </td>
                <UsageCells u={u} />
              </tr>
            ))}
            <tr data-testid="billing-usage-totals" className="border-t border-edge font-medium">
              <td className="px-4 py-2 text-fg">合计</td>
              <UsageCells u={usage.totals} />
            </tr>
          </tbody>
        </table>
      )}
    </SetCard>
  );
}

const ORDER_TONE: Record<string, Tone> = { pending: 'warn', paid: 'sage', cancelled: 'muted', failed: 'danger' };

/** 最近订单;待支付单可去支付(在线渠道)或取消。 */
export function OrdersCard({
  page,
  currency,
  onReload,
}: {
  page: OrdersPage;
  currency: string;
  onReload: () => void;
}) {
  const [busy, setBusy] = useState<number | null>(null);

  const cancel = async (o: Order) => {
    setBusy(o.id);
    try {
      await cancelOrder(o.id);
      toast('success', `订单 #${o.id} 已取消`);
      onReload();
    } catch (err) {
      toast('error', `取消订单失败:${errorMessage(err)}`);
    } finally {
      setBusy(null);
    }
  };

  return (
    <SetCard testId="billing-orders">
      {page.items.length === 0 ? (
        <div data-testid="billing-orders-empty" className="px-4 py-3 text-[11.5px] text-fg-4">
          暂无订单
        </div>
      ) : (
        page.items.map((o, i) => (
          <div
            key={o.id}
            data-testid={`billing-order-${o.id}`}
            className={cn('flex items-center gap-2.5 px-4 py-2.5', i > 0 && 'border-t border-edge')}
          >
            <div className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-[12.5px] text-fg">{orderSummary(o)}</span>
              <span className="truncate text-[11px] text-fg-4">
                #{o.id} · {formatTime(o.createdAt)} · {providerLabel(o.provider)}
                {o.creditMicros > 0 ? ` · 抵扣 ${formatMoney(o.creditMicros, currency)}` : ''}
              </span>
            </div>
            <span className="font-code text-[12px] text-fg">{formatMoney(o.amountMicros, currency)}</span>
            <Badge tone={ORDER_TONE[o.status] ?? 'muted'}>{orderStatusLabel(o.status)}</Badge>
            {o.status === 'pending' && (
              <>
                {safePayUrl(o.payUrl) && (
                  <SmBtn label="去支付" testId={`billing-order-pay-${o.id}`} onClick={() => openPayUrl(o.payUrl)} />
                )}
                <SmBtn
                  label={busy === o.id ? '取消中…' : '取消'}
                  testId={`billing-order-cancel-${o.id}`}
                  disabled={busy !== null}
                  onClick={() => void cancel(o)}
                />
              </>
            )}
          </div>
        ))
      )}
      {page.total > page.items.length && (
        <div className="border-t border-edge px-4 py-2 text-[11px] text-fg-4">
          共 {page.total} 张订单,显示最近 {page.items.length} 张
        </div>
      )}
    </SetCard>
  );
}
