import { useEffect, useMemo, useState } from 'react';
import { RefreshCw } from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  errorMessage, formatTime, getUsage, getUsageDaily,
  type UsageDay, type UsagePage, type UsageSummary,
} from '@/lib/accountApi';
import { SetCard, SmBtn } from './controls';
import { ACTIVITY_DAYS } from '@/lib/usageActivity';
import UsageActivity from './UsageActivity';

export { fillDays, recentUsageRange } from '@/lib/usageActivity';

export const USAGE_PAGE_SIZE = 10;
const numberFmt = new Intl.NumberFormat('en-US');

function count(value: number | undefined): string {
  return typeof value === 'number' && Number.isFinite(value) ? numberFmt.format(value) : '—';
}

export function cacheHitRate(input: number | undefined, cached: number | undefined): string {
  if (typeof input !== 'number' || typeof cached !== 'number' || !Number.isFinite(input) || !Number.isFinite(cached) || input <= 0 || cached < 0) return '—';
  return `${(Math.min(1, cached / input) * 100).toFixed(1)}%`;
}

function amount(value: number | undefined): string {
  return typeof value === 'number' && Number.isFinite(value)
    ? (value / 1_000_000).toFixed(6).replace(/(\.\d{2})0+$/, '$1')
    : '—';
}

function cost(value: number | undefined, currency: string): string {
  const text = amount(value);
  return text === '—' ? text : `${text} ${currency}`;
}

function LoadError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div role="alert" className="flex items-center gap-3 px-5 py-4 text-[12px] text-warn">
      <span className="min-w-0 flex-1">{message}</span>
      <SmBtn label="重试" onClick={onRetry} />
    </div>
  );
}

function UsageStats({ summary, currency }: { summary?: UsageSummary; currency: string }) {
  const metrics = [
    ['调用次数', count(summary?.requests)],
    ['输入 Token', count(summary?.inputTokens)],
    ['输出 Token', count(summary?.outputTokens)],
    ['缓存命中', cacheHitRate(summary?.inputTokens, summary?.cacheReadTokens)],
    ['缓存读取 / 写入', `${count(summary?.cacheReadTokens)} / ${count(summary?.cacheWriteTokens)}`],
    ['消耗费用', cost(summary?.costMicros, currency)],
  ];
  return (
    <div data-testid="usage-summary" className="grid grid-cols-2 gap-x-5 gap-y-5 border-b border-edge px-5 py-5 sm:grid-cols-3">
      {metrics.map(([label, value]) => (
        <div key={label} className="min-w-0">
          <div className="mb-1.5 text-[11px] text-fg-3">{label}</div>
          <div className="break-words font-code text-[18px] leading-6 tracking-tight text-fg tabular-nums">{value}</div>
        </div>
      ))}
    </div>
  );
}

/** Summary is returned for the full range by the backend; never sum a paginated slice as the total. */
export default function UsageDashboard({ currency, from, to, chart = false, details = true, testId = 'account-usage-card' }: {
  currency: string; from: string; to: string; chart?: boolean; details?: boolean; testId?: string;
}) {
  const scope = `${from}\u0000${to}`;
  const [snapshot, setSnapshot] = useState<{ scope: string; page: UsagePage } | null>(null);
  const page = snapshot?.scope === scope ? snapshot.page : null;
  const [daily, setDaily] = useState<UsageDay[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dailyError, setDailyError] = useState<string | null>(null);
  const [offset, setOffset] = useState(0);
  const [loading, setLoading] = useState(true);
  const [reload, setReload] = useState(0);
  const retry = () => setReload((value) => value + 1);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    setError(null);
    getUsage({ from, to, limit: details ? USAGE_PAGE_SIZE : 1, offset: details ? offset : 0 }).then((result) => {
      if (alive) setSnapshot({ scope, page: { ...result, items: Array.isArray(result.items) ? result.items : [], total: result.total ?? 0 } });
    }).catch((err) => { if (alive) setError(errorMessage(err)); })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [scope, from, to, details, offset, reload]);

  useEffect(() => {
    if (!chart) return;
    let alive = true;
    getUsageDaily(ACTIVITY_DAYS).then((result) => {
      if (alive) { setDaily(Array.isArray(result.items) ? result.items : []); setDailyError(null); }
    }).catch((err) => { if (alive) setDailyError(errorMessage(err)); });
    return () => { alive = false; };
  }, [chart, reload]);

  const current = Math.floor(offset / USAGE_PAGE_SIZE) + 1;
  const pages = Math.max(1, Math.ceil((page?.total ?? 0) / USAGE_PAGE_SIZE));
  const hasData = page !== null && !error;
  const endExclusive = Date.parse(to);
  const lastDate = Number.isFinite(endExclusive) ? new Date(endExclusive - 1).toISOString().slice(0, 10) : to.slice(0, 10);
  const activityToday = useMemo(() => Number.isFinite(Date.parse(to)) ? new Date(Date.parse(to) - 1) : new Date(), [to]);
  return (
    <SetCard testId={testId}>
      <div className="flex items-center justify-between gap-3 border-b border-edge px-5 py-3.5">
        <div className="min-w-0">
          <div className="text-[13px] font-medium text-fg">{chart ? '近 30 天用量' : '本周期用量'}</div>
          <div className="mt-1 text-[10.5px] text-fg-4">{from.slice(0, 10)} — {lastDate} · 云端调用记录</div>
        </div>
        <SmBtn testId="usage-refresh" label={<><RefreshCw size={11} />刷新</>} disabled={loading} onClick={retry} />
      </div>
      <UsageStats summary={hasData ? page.summary : undefined} currency={currency} />
      {!details && error && <LoadError message={`用量汇总加载失败：${error}`} onRetry={retry} />}
      {chart && (dailyError ? <LoadError message={`Token 活动加载失败：${dailyError}`} onRetry={retry} />
        : daily ? <UsageActivity items={daily} today={activityToday} /> : <div className="px-5 py-6 text-[12px] text-fg-4">正在读取 Token 活动…</div>)}
      {details && <>
      <div className="px-5 pb-2 pt-4 text-[12px] font-medium text-fg-2">逐次模型调用</div>
      <div className="px-5 pb-3 text-[10.5px] text-fg-4">缓存命中率按缓存读取 Token / 输入 Token 计算；费用保留实际精度。</div>
      {error ? <LoadError message={`用量明细加载失败：${error}`} onRetry={retry} /> : (
        <>
          <div className="overflow-x-auto" aria-busy={loading}>
            <table data-testid="usage-calls-table" className="w-full min-w-[760px] text-[11px] tabular-nums">
              <caption className="sr-only">每次模型调用的 Token、缓存、耗时和费用</caption>
              <thead className="bg-shell-panel text-[10.5px] text-fg-3">
                <tr>{['时间', '模型 / 状态', '输入', '输出', '缓存读取', '缓存写入', '命中率', '耗时', `费用 · ${currency}`].map((label, index) => (
                  <th key={label} scope="col" className={cn('whitespace-nowrap px-2 py-2.5 font-normal', index < 2 ? 'text-left' : 'text-right', index === 0 && 'pl-5')}>{label}</th>
                ))}</tr>
              </thead>
              <tbody>
                {loading ? <tr><td colSpan={9} className="px-5 py-6 text-fg-4">正在读取调用记录…</td></tr>
                  : page?.items.length ? page.items.map((item) => (
                    <tr key={item.id} data-testid={`usage-call-${item.id}`} className="border-t border-edge text-fg-2 hover:bg-shell-hover">
                      <td title={formatTime(item.createdAt)} className="whitespace-nowrap py-3 pl-5 pr-2 text-[10px] leading-4 text-fg-3">
                        <span className="block">{formatTime(item.createdAt).split(' ')[0]}</span>
                        <span>{formatTime(item.createdAt).split(' ').slice(1).join(' ')}</span>
                      </td>
                      <td className="max-w-[180px] px-2 py-3">
                        <div className="truncate font-code" title={item.model}>{item.model}</div>
                        <div className={cn('mt-0.5 text-[10px]', item.status === 'error' ? 'text-warn' : 'text-fg-4')}>
                          {item.status === 'ok' ? '成功' : item.status === 'error' ? item.errorCode || '失败' : item.status || '未知'}
                          {item.endpoint ? ` · ${item.endpoint}` : ''}
                        </div>
                      </td>
                      {[item.inputTokens, item.outputTokens, item.cacheReadTokens, item.cacheWriteTokens].map((value, index) => (
                        <td key={index} className="whitespace-nowrap px-2 py-3 text-right font-code">{count(value)}</td>
                      ))}
                      <td className="whitespace-nowrap px-2 py-3 text-right font-code">{cacheHitRate(item.inputTokens, item.cacheReadTokens)}</td>
                      <td className="whitespace-nowrap px-2 py-3 text-right font-code">{Number.isFinite(item.latencyMs) ? `${(item.latencyMs / 1000).toFixed(2)}s` : '—'}</td>
                      <td title={cost(item.costMicros, currency)} className="whitespace-nowrap px-2 py-3 text-right font-code">{amount(item.costMicros)}</td>
                    </tr>
                  )) : <tr><td colSpan={9} data-testid="account-usage-empty" className="px-5 py-6 text-center text-fg-4">暂无调用记录</td></tr>}
              </tbody>
            </table>
          </div>
          {!!page && page.total > USAGE_PAGE_SIZE && (
            <div className="flex items-center justify-end gap-2 border-t border-edge px-5 py-3 text-[11px] text-fg-3">
              <span data-testid="account-usage-page">第 {current} / {pages} 页 · 共 {page.total} 条</span>
              <SmBtn label="上一页" testId="account-usage-prev" disabled={loading || offset === 0} onClick={() => setOffset((value) => Math.max(0, value - USAGE_PAGE_SIZE))} />
              <SmBtn label="下一页" testId="account-usage-next" disabled={loading || current >= pages} onClick={() => setOffset((value) => value + USAGE_PAGE_SIZE)} />
            </div>
          )}
        </>
      )}
      </>}
    </SetCard>
  );
}
