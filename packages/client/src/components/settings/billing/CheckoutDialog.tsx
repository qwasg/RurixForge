import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { cn } from '@/lib/cn';
import { errorMessage, formatMoney, formatTime } from '@/lib/accountApi';
import {
  intervalLabel,
  modeLabel,
  postCheckout,
  providerLabel,
  shortfallFromError,
  type BillingInterval,
  type CheckoutResult,
  type PaymentInfo,
  type Quote,
  type Shortfall,
} from '@/lib/billingApi';
import { useSettingsStore } from '@/lib/settingsStore';
import { SmBtn } from '../controls';

/** 报价的生效说明(15 §11.1 报价规则)。 */
export function quoteNote(q: Pick<Quote, 'mode' | 'planName' | 'startsAt' | 'endsAt'>): string {
  switch (q.mode) {
    case 'upgrade':
      return `立即生效,至 ${formatTime(q.endsAt)}。当前已购档位按剩余时长折算抵扣,原订阅随即作废。`;
    case 'renew':
      return `接在当前订阅之后,${formatTime(q.startsAt)} 起生效,至 ${formatTime(q.endsAt)}。`;
    case 'downgrade':
      return `当前档位继续用到期,${formatTime(q.startsAt)} 起切换为「${q.planName}」,至 ${formatTime(q.endsAt)}。`;
    default:
      return `立即生效,至 ${formatTime(q.endsAt)};套餐内额度每月重置。`;
  }
}

function Line({
  label,
  value,
  testId,
  strong = false,
  positive = false,
}: {
  label: ReactNode;
  value: ReactNode;
  testId?: string;
  strong?: boolean;
  positive?: boolean;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="min-w-0 flex-1 text-fg-3">{label}</span>
      <span
        data-testid={testId}
        className={cn('font-code', strong ? 'text-[13px] font-semibold text-fg' : positive ? 'text-sage' : 'text-fg-2')}
      >
        {value}
      </span>
    </div>
  );
}

/**
 * 购买确认弹窗:展示只读报价(标价 / 升级抵扣 / 应付 / 退回余额 / 生效时间),选支付方式后才下单。
 * 余额支付要求余额 ≥ 应付(不扣成负数);服务端 402 的差额(顶层 balanceMicros / amountMicros)同样就地展示。
 */
export default function CheckoutDialog({
  quote,
  interval,
  payment,
  onClose,
  onDone,
}: {
  quote: Quote;
  interval: BillingInterval;
  payment: PaymentInfo;
  onClose: () => void;
  onDone: (result: CheckoutResult) => void;
}) {
  const providers = useMemo(() => ['balance', ...(payment.enabled ? payment.providers : [])], [payment]);
  const [provider, setProvider] = useState('balance');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [serverShort, setServerShort] = useState<Shortfall | null>(null);
  const cur = quote.currency || 'USD';
  const needsPay = quote.amountMicros > 0;

  // Esc 只关弹窗:在捕获阶段拦下,免得全局 Esc 顺带关掉整个设置浮层。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.stopPropagation();
      if (!busy) onClose();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [busy, onClose]);

  const localShort: Shortfall | null =
    needsPay && provider === 'balance' && quote.amountMicros > quote.balanceMicros
      ? {
          balanceMicros: quote.balanceMicros,
          amountMicros: quote.amountMicros,
          shortMicros: quote.amountMicros - quote.balanceMicros,
        }
      : null;
  const short = serverShort ?? localShort;

  const confirm = async () => {
    setBusy(true);
    setError(null);
    setServerShort(null);
    try {
      onDone(await postCheckout({ planId: quote.planId, interval, provider: needsPay ? provider : 'balance' }));
    } catch (err) {
      const s = shortfallFromError(err);
      if (s) setServerShort(s);
      else setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const title = `${modeLabel(quote.mode)}「${quote.planName}」· ${intervalLabel(interval)}`;
  const amount = formatMoney(quote.amountMicros, cur);
  const confirmLabel = busy
    ? '处理中…'
    : !needsPay
      ? '确认'
      : provider === 'balance'
        ? `确认支付 ${amount}`
        : `前往支付 ${amount}`;

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/30 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        data-testid="billing-checkout-dialog"
        className="forge-pop-in flex w-[420px] max-w-full flex-col gap-3 rounded-xl border border-edge-strong bg-shell-float p-5 text-fg shadow-float"
      >
        <div className="text-[14px] font-medium">{title}</div>
        <div data-testid="billing-checkout-note" className="text-[11.5px] leading-[17px] text-fg-3">
          {quoteNote(quote)}
        </div>
        <div className="flex flex-col gap-1.5 rounded-lg border border-edge bg-shell-sunk px-3 py-2.5 text-[12px]">
          <Line label="标价" value={formatMoney(quote.listPriceMicros, cur)} testId="billing-checkout-list" />
          {quote.creditMicros > 0 && (
            <Line label="升级抵扣" value={`−${formatMoney(quote.creditMicros, cur)}`} testId="billing-checkout-credit" positive />
          )}
          <Line label="应付" value={amount} testId="billing-checkout-amount" strong />
          {quote.refundMicros > 0 && (
            <Line
              label="生效时退回余额"
              value={`+${formatMoney(quote.refundMicros, cur)}`}
              testId="billing-checkout-refund"
              positive
            />
          )}
          <Line label="当前余额" value={formatMoney(quote.balanceMicros, cur)} />
        </div>
        {needsPay && providers.length > 1 && (
          <div className="flex flex-col gap-1.5 text-[11.5px] text-fg-3">
            支付方式
            <div className="flex flex-wrap gap-1.5">
              {providers.map((p) => (
                <button
                  key={p}
                  type="button"
                  aria-pressed={provider === p}
                  data-testid={`billing-checkout-provider-${p}`}
                  onClick={() => {
                    setProvider(p);
                    setServerShort(null);
                    setError(null);
                  }}
                  className={cn(
                    'flex h-[26px] items-center rounded-md border px-2.5 text-[12px] transition-colors',
                    provider === p
                      ? 'border-acc bg-acc-bg text-acc'
                      : 'border-edge bg-shell-panel text-fg-2 hover:bg-shell-hover',
                  )}
                >
                  {providerLabel(p)}
                </button>
              ))}
            </div>
          </div>
        )}
        {short && (
          <div
            data-testid="billing-checkout-shortfall"
            className="flex items-center gap-2 rounded-lg border border-edge bg-warn-bg px-3 py-2 text-[11.5px] text-warn"
          >
            <span className="min-w-0 flex-1">
              余额不足,还差 {formatMoney(short.shortMicros, cur)}(应付 {formatMoney(short.amountMicros, cur)},余额{' '}
              {formatMoney(short.balanceMicros, cur)})
            </span>
            <SmBtn
              label="去充值"
              testId="billing-checkout-recharge"
              onClick={() => {
                onClose();
                useSettingsStore.getState().setPage('account');
              }}
            />
          </div>
        )}
        {error && (
          <div data-testid="billing-checkout-error" className="text-[11.5px] text-danger">
            {error}
          </div>
        )}
        <div className="flex justify-end gap-1.5 pt-1">
          <SmBtn label="取消" testId="billing-checkout-cancel" disabled={busy} onClick={onClose} />
          <SmBtn
            accent
            label={confirmLabel}
            testId="billing-checkout-confirm"
            disabled={busy || (provider === 'balance' && short !== null)}
            onClick={() => void confirm()}
          />
        </div>
      </div>
    </div>
  );
}
