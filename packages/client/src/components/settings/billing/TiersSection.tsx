import type { ReactNode } from 'react';
import { Check } from 'lucide-react';
import { cn } from '@/lib/cn';
import { formatMoney } from '@/lib/accountApi';
import {
  isFreeTier,
  perMonthMicros,
  savingsPercent,
  tierPurchasable,
  type BillingInterval,
  type Tier,
  type TierAction,
} from '@/lib/billingApi';
import { SmBtn } from '../controls';
import { Badge } from './parts';

/** 卡片上的价格:按年时显示折合每月价 + 年付总价与节省比例。 */
export function tierPrice(t: Tier, interval: BillingInterval, currency: string): { main: string; unit: string; note: string } {
  if (isFreeTier(t)) return { main: '免费', unit: '', note: '无需付费' };
  if (interval === 'year') {
    if (t.priceYearlyMicros <= 0) return { main: '—', unit: '', note: '暂不支持按年购买' };
    const pct = savingsPercent(t.priceMonthlyMicros, t.priceYearlyMicros);
    return {
      main: formatMoney(perMonthMicros(t.priceYearlyMicros), currency),
      unit: '/ 月',
      note: `按年付 ${formatMoney(t.priceYearlyMicros, currency)}${pct > 0 ? `,省 ${pct}%` : ''}`,
    };
  }
  if (t.priceMonthlyMicros <= 0) return { main: '—', unit: '', note: '暂不支持按月购买' };
  return { main: formatMoney(t.priceMonthlyMicros, currency), unit: '/ 月', note: '按月付费,可随时升级' };
}

/** 未登录 / 会员信息未到时的按钮:免费档不给按钮。 */
export function guestAction(t: Tier, loggedIn: boolean): TierAction {
  if (isFreeTier(t)) return { kind: 'free', label: null, current: false };
  return { kind: 'new', label: loggedIn ? '开通' : '登录后开通', current: false };
}

export function IntervalSwitch({
  value,
  onChange,
  savePct,
}: {
  value: BillingInterval;
  onChange: (v: BillingInterval) => void;
  savePct: number;
}) {
  const opt = (v: BillingInterval, label: ReactNode) => (
    <button
      type="button"
      aria-pressed={value === v}
      data-testid={`billing-interval-${v}`}
      onClick={() => onChange(v)}
      className={cn(
        'flex h-[24px] items-center gap-1 rounded-[5px] px-2.5 text-[12px] transition-colors',
        value === v ? 'bg-shell-panel text-fg shadow-[0_1px_2px_rgba(0,0,0,0.08)]' : 'text-fg-3 hover:text-fg-2',
      )}
    >
      {label}
    </button>
  );
  return (
    <div role="group" aria-label="付费周期" className="flex items-center gap-0.5 rounded-md border border-edge bg-shell-sunk p-0.5">
      {opt('month', '按月')}
      {opt(
        'year',
        <>
          按年
          {savePct > 0 && <span className="text-[11px] text-sage">省 {savePct}%</span>}
        </>,
      )}
    </div>
  );
}

function Feature({ children }: { children: ReactNode }) {
  return (
    <li className="flex items-start gap-1.5">
      <Check size={12} className="mt-[2px] shrink-0 text-sage" />
      <span className="min-w-0">{children}</span>
    </li>
  );
}

function QuotaLine({ label, value, testId }: { label: string; value: string; testId?: string }) {
  return (
    <div className="flex items-center gap-2">
      <span className="min-w-0 flex-1 text-fg-3">{label}</span>
      <span data-testid={testId} className="font-code text-fg">
        {value}
      </span>
    </div>
  );
}

/** 一张档位卡(参照 Cursor 价格页):价格、每月套餐内额度(两池)、权益、开通/升级/降级/续订按钮。 */
export function TierCard({
  t,
  action,
  interval,
  currency,
  quoting,
  disabled,
  onPick,
}: {
  t: Tier;
  action: TierAction;
  interval: BillingInterval;
  currency: string;
  /** 正在报价的档位(tier 标识);报价期间所有按钮禁用。 */
  quoting: string | null;
  disabled: boolean;
  onPick: (t: Tier) => void;
}) {
  const price = tierPrice(t, interval, currency);
  const purchasable = tierPurchasable(t, interval);
  return (
    <div
      data-testid={`billing-tier-${t.tier}`}
      className={cn(
        'flex flex-col gap-2 rounded-[10px] border bg-shell-sunk p-4',
        t.highlight ? 'border-acc' : 'border-edge',
      )}
    >
      <div className="flex items-center gap-1.5">
        <span className="text-[14px] font-semibold text-fg">{t.name}</span>
        {t.highlight && <Badge tone="acc">推荐</Badge>}
        <span className="flex-1" />
        {action.current && (
          <Badge tone="sage" testId={`billing-tier-current-${t.tier}`}>
            当前档位
          </Badge>
        )}
      </div>
      {t.tagline && <div className="text-[11.5px] text-fg-3">{t.tagline}</div>}
      <div className="flex items-baseline gap-1">
        <span data-testid={`billing-tier-price-${t.tier}`} className="font-code text-[18px] font-semibold text-fg">
          {price.main}
        </span>
        {price.unit && <span className="text-[11px] text-fg-3">{price.unit}</span>}
      </div>
      <div data-testid={`billing-tier-note-${t.tier}`} className="-mt-1 text-[11px] text-fg-4">
        {price.note}
      </div>
      <div className="flex flex-col gap-0.5 rounded-md border border-edge bg-shell-panel px-2.5 py-2 text-[11.5px]">
        <div className="text-[10.5px] text-fg-4">每月套餐内额度</div>
        <QuotaLine
          label="第三方模型(按 API 价)"
          value={t.includedApiMicros > 0 ? formatMoney(t.includedApiMicros, currency) : '按量付费'}
          testId={`billing-tier-api-${t.tier}`}
        />
        <QuotaLine
          label="平台模型"
          value={t.includedForgeMicros > 0 ? formatMoney(t.includedForgeMicros, currency) : '—'}
          testId={`billing-tier-forge-${t.tier}`}
        />
      </div>
      {(t.features.length > 0 || t.dailyLimitMicros > 0) && (
        <ul className="flex flex-col gap-1 text-[11.5px] leading-[16px] text-fg-2">
          {t.dailyLimitMicros > 0 && <Feature>每日上限 {formatMoney(t.dailyLimitMicros, currency)}</Feature>}
          {t.features.map((f) => (
            <Feature key={f}>{f}</Feature>
          ))}
        </ul>
      )}
      <div className="mt-auto pt-1 [&>button]:w-full">
        {action.label ? (
          <SmBtn
            accent={action.kind === 'new' || action.kind === 'upgrade'}
            label={quoting === t.tier ? '报价中…' : action.label}
            testId={`billing-tier-action-${t.tier}`}
            disabled={!purchasable || disabled || quoting !== null}
            onClick={() => onPick(t)}
          />
        ) : null}
      </div>
    </div>
  );
}
