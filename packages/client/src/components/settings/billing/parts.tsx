import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { cn } from '@/lib/cn';
import { errorMessage, formatMoney } from '@/lib/accountApi';
import { safePayUrl } from '@/lib/billingApi';
import { useToastStore } from '@/lib/toastStore';
import { SmBtn } from '../controls';

/** 「套餐与用量」页(D-042,15 §11.4)共用的小部件与工具。 */

export function toast(kind: 'success' | 'error' | 'info', msg: string): void {
  useToastStore.getState().push(kind, msg);
}

/** RFC3339 → 本地日期(不含时间);非法值原样返回。 */
export function formatDay(ts: string | null | undefined): string {
  if (!ts) return '—';
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return d.toLocaleDateString('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit' });
}

/** 打开在线渠道的支付页(只放行 http/https);地址不可用返回 false。 */
export function openPayUrl(url: string | null | undefined): boolean {
  const safe = safePayUrl(url);
  if (!safe) return false;
  window.open(safe, '_blank', 'noopener,noreferrer');
  return true;
}

export type Tone = 'sage' | 'warn' | 'muted' | 'acc' | 'danger';

export function Badge({ tone, children, testId }: { tone: Tone; children: ReactNode; testId?: string }) {
  return (
    <span
      data-testid={testId}
      className={cn(
        'flex h-[18px] shrink-0 items-center gap-0.5 rounded-full px-1.5 text-[10px] font-normal',
        tone === 'sage' && 'bg-sage-bg text-sage',
        tone === 'warn' && 'bg-warn-bg text-warn',
        tone === 'acc' && 'bg-acc-bg text-acc',
        tone === 'danger' && 'bg-danger-bg text-danger',
        tone === 'muted' && 'bg-shell-active text-fg-3',
      )}
    >
      {children}
    </span>
  );
}

export function SectionError({ message, onRetry, testId }: { message: string; onRetry?: () => void; testId?: string }) {
  return (
    <div data-testid={testId} className="flex items-center gap-2 px-4 py-3 text-[11.5px] text-warn">
      <span className="min-w-0 flex-1">{message}</span>
      {onRetry && <SmBtn label="重试" onClick={onRetry} />}
    </div>
  );
}

export function Loading() {
  return <div className="px-4 py-3 text-[11.5px] text-fg-4">加载中…</div>;
}

/** 额度进度条:已用 / 总额(总额为 0 时显示 emptyText)。 */
export function UsageBar({
  label,
  used,
  total,
  currency,
  emptyText = '未包含',
  testId,
}: {
  label: ReactNode;
  used: number;
  total: number;
  currency: string;
  emptyText?: string;
  testId?: string;
}) {
  const pct = total > 0 ? Math.min(100, Math.max(0, (used / total) * 100)) : used > 0 ? 100 : 0;
  const text =
    total > 0 ? `${formatMoney(used, currency)} / ${formatMoney(total, currency)}` : used > 0 ? formatMoney(used, currency) : emptyText;
  return (
    <div data-testid={testId} className="flex flex-col gap-1">
      <div className="flex items-center gap-2 text-[11.5px]">
        <span className="min-w-0 flex-1 text-fg-2">{label}</span>
        <span className="font-code text-[11px] text-fg-3">{text}</span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-shell-active">
        <div
          className={cn('h-full rounded-full', pct >= 100 ? 'bg-danger' : pct >= 80 ? 'bg-warn' : 'bg-sage')}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

export interface Loaded<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
  reload: () => Promise<void>;
  /** 用写操作的回包直接替换数据,并作废仍在路上的旧请求。 */
  setData: (data: T) => void;
}

/** 按需加载一段数据:enabled 变 true 时拉取,变 false 时清空(换账号不串数据)。 */
export function useLoader<T>(load: () => Promise<T>, enabled: boolean): Loaded<T> {
  const [state, setState] = useState<{ data: T | null; error: string | null; loading: boolean }>({
    data: null,
    error: null,
    loading: enabled,
  });
  const loadRef = useRef(load);
  loadRef.current = load;
  const seq = useRef(0);

  const reload = useCallback(async () => {
    const id = ++seq.current;
    setState((s) => ({ ...s, loading: true }));
    try {
      const data = await loadRef.current();
      if (id === seq.current) setState({ data, error: null, loading: false });
    } catch (err) {
      if (id === seq.current) setState((s) => ({ ...s, error: errorMessage(err), loading: false }));
    }
  }, []);

  const setData = useCallback((data: T) => {
    seq.current += 1;
    setState({ data, error: null, loading: false });
  }, []);

  useEffect(() => {
    if (enabled) {
      void reload();
    } else {
      seq.current += 1;
      setState({ data: null, error: null, loading: false });
    }
  }, [enabled, reload]);

  return { ...state, reload, setData };
}
