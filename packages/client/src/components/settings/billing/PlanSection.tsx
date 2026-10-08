import { useEffect, useState } from 'react';
import { ExternalLink } from 'lucide-react';
import { errorMessage, formatMoney, formatTime } from '@/lib/accountApi';
import { useAccountStore } from '@/lib/accountStore';
import {
  cancelOrder,
  cancelScheduled,
  intervalLabel,
  microsToPlain,
  orderSummary,
  parseAmountToMicros,
  patchOnDemand,
  poolLabel,
  safePayUrl,
  type Membership,
  type MembershipSubscription,
  type OnDemand,
  type Order,
} from '@/lib/billingApi';
import { SetCard, SetInput, SetRow, SetToggle, SmBtn } from '../controls';
import { Badge, formatDay, openPayUrl, toast, UsageBar } from './parts';

const SOURCE_TEXT: Record<string, string> = { grant: '后台开通', redeem: '兑换码开通' };

/** 当前档位:余额、待支付订单、已预约(降级 / 续订)、本周期两池额度与额度包。 */
export function PlanCard({
  m,
  onMembership,
  onReload,
}: {
  m: Membership;
  onMembership: (m: Membership) => void;
  onReload: () => void;
}) {
  const refreshStatus = useAccountStore((st) => st.refreshStatus);
  const [confirmId, setConfirmId] = useState<number | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const sub = m.subscription;
  const cur = m.currency;
  const pending = m.pendingOrder;
  const lastScheduledId = m.scheduled.length > 0 ? m.scheduled[m.scheduled.length - 1].id : null;

  const cancelSchedule = async (s: MembershipSubscription) => {
    setBusy(`scheduled-${s.id}`);
    try {
      onMembership(await cancelScheduled(s.id));
      setConfirmId(null);
      toast('success', `已取消预约「${s.planName}」,${formatMoney(s.valueMicros ?? 0, cur)} 已退回余额`);
      void refreshStatus();
    } catch (err) {
      toast('error', `取消预约失败:${errorMessage(err)}`);
    } finally {
      setBusy(null);
    }
  };

  const cancelPending = async (o: Order) => {
    setBusy(`order-${o.id}`);
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

  const desc = sub
    ? [`订阅至 ${formatTime(sub.endsAt)}`, sub.source ? SOURCE_TEXT[sub.source] : ''].filter(Boolean).join(' · ')
    : '免费档:每月(UTC 自然月)含少量平台模型额度,第三方模型按量付费';

  return (
    <SetCard testId="billing-plan-card">
      <SetRow
        title={
          <>
            <span data-testid="billing-current-tier">{m.tier.name || m.tier.tier}</span>
            {sub?.billingInterval ? <Badge tone="acc">{intervalLabel(sub.billingInterval)}</Badge> : null}
            {!sub && <Badge tone="muted">免费</Badge>}
          </>
        }
        desc={desc}
        control={
          <div className="flex flex-col items-end">
            <span className="text-[10.5px] text-fg-4">余额</span>
            <span data-testid="billing-balance" className="font-code text-[15px] font-semibold text-fg">
              {formatMoney(m.balanceMicros, cur)}
            </span>
          </div>
        }
      />
      {pending && (
        <div data-testid="billing-pending-order" className="flex items-center gap-2 border-b border-edge px-4 py-2.5">
          <Badge tone="warn">待支付</Badge>
          <span className="min-w-0 flex-1 truncate text-[12px] text-fg-2">
            {orderSummary(pending)} · 应付 {formatMoney(pending.amountMicros, cur)}
          </span>
          {safePayUrl(pending.payUrl) && (
            <SmBtn
              label={
                <>
                  <ExternalLink size={11} />
                  去支付
                </>
              }
              testId="billing-pending-pay"
              onClick={() => openPayUrl(pending.payUrl)}
            />
          )}
          <SmBtn
            label={busy === `order-${pending.id}` ? '取消中…' : '取消订单'}
            testId="billing-pending-cancel"
            disabled={busy !== null}
            onClick={() => void cancelPending(pending)}
          />
        </div>
      )}
      {m.scheduled.map((s) => (
        <div key={s.id} data-testid={`billing-scheduled-${s.id}`} className="flex items-center gap-2 border-b border-edge px-4 py-2.5">
          <Badge tone="acc">已预约</Badge>
          <span className="min-w-0 flex-1 truncate text-[12px] text-fg-2">
            {[s.planName, intervalLabel(s.billingInterval)].filter(Boolean).join(' · ')} · {formatTime(s.startsAt)} 起生效
          </span>
          {s.id === lastScheduledId &&
            (confirmId === s.id ? (
              <>
                <SmBtn label="保留" onClick={() => setConfirmId(null)} />
                <SmBtn
                  accent
                  label={busy === `scheduled-${s.id}` ? '取消中…' : `取消并退回 ${formatMoney(s.valueMicros ?? 0, cur)}`}
                  testId={`billing-scheduled-cancel-confirm-${s.id}`}
                  disabled={busy !== null}
                  onClick={() => void cancelSchedule(s)}
                />
              </>
            ) : (
              <SmBtn
                label="取消预约"
                testId={`billing-scheduled-cancel-${s.id}`}
                disabled={busy !== null}
                onClick={() => setConfirmId(s.id)}
              />
            ))}
        </div>
      ))}
      <div data-testid="billing-pools" className="flex flex-col gap-2.5 border-b border-edge px-4 py-3.5">
        <div className="flex items-center gap-2 text-[11px] text-fg-3">
          <span className="min-w-0 flex-1">本周期套餐内额度</span>
          <span data-testid="billing-cycle">
            {formatDay(m.cycle.start)} – {formatDay(m.cycle.end)} · 到期自动重置
          </span>
        </div>
        <UsageBar
          label={poolLabel('api')}
          used={m.pools.api.usedMicros}
          total={m.pools.api.includedMicros}
          currency={cur}
          emptyText="不含,按量付费"
          testId="billing-pool-api"
        />
        <UsageBar
          label={poolLabel('forge')}
          used={m.pools.forge.usedMicros}
          total={m.pools.forge.includedMicros}
          currency={cur}
          testId="billing-pool-forge"
        />
      </div>
      {m.packs.map((p) => (
        <div key={p.id} data-testid={`billing-pack-${p.id}`} className="flex flex-col gap-2 border-b border-edge px-4 py-3">
          <div className="flex items-center gap-2 text-[12px]">
            <span className="font-medium text-fg">{p.planName}</span>
            <Badge tone="muted">{p.tier ? '附加档位' : '额度包'}</Badge>
            <span className="flex-1" />
            <span className="text-[11px] text-fg-4">到期 {formatTime(p.endsAt)}</span>
          </div>
          {(p.quotaMicros > 0 || p.usedMicros > 0) && (
            <UsageBar label={poolLabel('api')} used={p.usedMicros} total={p.quotaMicros} currency={cur} />
          )}
          {((p.forgeQuotaMicros ?? 0) > 0 || (p.forgeUsedMicros ?? 0) > 0) && (
            <UsageBar label={poolLabel('forge')} used={p.forgeUsedMicros ?? 0} total={p.forgeQuotaMicros ?? 0} currency={cur} />
          )}
        </div>
      ))}
      <div className="px-4 py-2.5 text-[11px] leading-[16px] text-fg-4">
        先扣模型所在池的套餐内额度(额度包按到期先后),用尽后按量付费从余额扣除。
      </div>
    </SetCard>
  );
}

/** 按量付费:开关(默认开)与每周期上限(0 = 不设上限)。 */
export function OnDemandCard({
  od,
  currency,
  onChange,
}: {
  od: OnDemand;
  currency: string;
  onChange: (od: OnDemand) => void;
}) {
  const [limit, setLimit] = useState(od.limitMicros > 0 ? microsToPlain(od.limitMicros) : '');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setLimit(od.limitMicros > 0 ? microsToPlain(od.limitMicros) : '');
  }, [od.limitMicros]);

  const apply = async (patch: { enabled?: boolean; limitMicros?: number }, done: string) => {
    setBusy(true);
    setError(null);
    try {
      onChange(await patchOnDemand(patch));
      toast('success', done);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const saveLimit = () => {
    const raw = limit.trim();
    const micros = raw === '' ? 0 : parseAmountToMicros(raw);
    if (micros === null) {
      setError('上限应为 0 – 1,000,000 之间的金额,最多 6 位小数');
      return;
    }
    if (micros === od.limitMicros) {
      setError(null);
      return;
    }
    void apply(
      { limitMicros: micros },
      micros === 0 ? '已取消按量付费上限' : `按量付费上限已设为 ${formatMoney(micros, currency)}`,
    );
  };

  return (
    <SetCard testId="billing-ondemand-card">
      <SetRow
        title="按量付费"
        desc="套餐内额度用尽后继续使用,按实际用量从余额扣费(第三方模型按 API 价)。关闭后额度用尽即暂停。"
        control={
          <SetToggle
            on={od.enabled}
            testId="billing-ondemand-toggle"
            ariaLabel="按量付费"
            onChange={(v) => {
              if (!busy) void apply({ enabled: v }, v ? '已开启按量付费' : '已关闭按量付费');
            }}
          />
        }
      />
      <SetRow
        dim={!od.enabled}
        last
        title="每周期上限"
        desc={`本周期已用 ${formatMoney(od.usedMicros, currency)}${
          od.limitMicros > 0 ? ` / ${formatMoney(od.limitMicros, currency)}` : ',未设上限'
        };留空或 0 = 不设上限`}
        control={
          <div className="flex items-center gap-1.5">
            <SetInput value={limit} onChange={setLimit} width={110} placeholder="不限" testId="billing-ondemand-limit" />
            <span className="text-[11px] text-fg-3">{currency}</span>
            <SmBtn label={busy ? '保存中…' : '保存'} testId="billing-ondemand-save" disabled={busy} onClick={saveLimit} />
          </div>
        }
      />
      {od.limitMicros > 0 && (
        <div className="px-4 pb-3">
          <UsageBar
            label="本周期按量付费"
            used={od.usedMicros}
            total={od.limitMicros}
            currency={currency}
            testId="billing-ondemand-usage"
          />
        </div>
      )}
      {error && (
        <div data-testid="billing-ondemand-error" className="px-4 pb-3 text-[11.5px] text-danger">
          {error}
        </div>
      )}
    </SetCard>
  );
}
