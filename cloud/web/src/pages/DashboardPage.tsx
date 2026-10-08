import { Activity, Coins, KeyRound, RefreshCw, Users, Zap, type LucideIcon } from 'lucide-react';
import { useMemo, useState, type ReactNode } from 'react';
import { StatusDot } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardHeader, ErrorBlock, LoadingBlock, PageHeader, SegmentedControl } from '@/components/ui/misc';
import { Table, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { adminApi } from '@/lib/api/admin';
import type { DashboardDay } from '@/lib/api/types';
import { formatAmount, formatCompact, formatMoney, formatNumber } from '@/lib/format';
import { useAsync } from '@/lib/hooks';
import { useSettings } from '@/lib/settings';

function StatCard({ icon: Icon, label, value, sub }: { icon: LucideIcon; label: string; value: ReactNode; sub?: ReactNode }) {
  return (
    <Card className="p-4">
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <Icon className="size-4" />
        {label}
      </div>
      <div className="tabular mt-2 text-2xl font-semibold leading-8">{value}</div>
      {sub ? <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">{sub}</div> : null}
    </Card>
  );
}

type Metric = 'requests' | 'tokens' | 'cost';

function DailyChart({ days, currency }: { days: DashboardDay[]; currency: string }) {
  const [metric, setMetric] = useState<Metric>('requests');
  const data = useMemo(() => [...days].sort((a, b) => a.date.localeCompare(b.date)).slice(-14), [days]);
  const valueOf = (d: DashboardDay) =>
    metric === 'requests' ? d.requests : metric === 'tokens' ? d.inputTokens + d.outputTokens : d.costMicros;
  const max = Math.max(0, ...data.map(valueOf));
  const axis = (v: number) => (metric === 'cost' ? formatAmount(v) : formatCompact(v));

  return (
    <Card>
      <CardHeader
        title="近 14 天"
        actions={
          <SegmentedControl<Metric>
            aria-label="图表指标"
            value={metric}
            onChange={setMetric}
            options={[
              { value: 'requests', label: '请求数' },
              { value: 'tokens', label: 'Token' },
              { value: 'cost', label: `费用（${currency}）` },
            ]}
          />
        }
      />
      {data.length === 0 ? (
        <p className="py-16 text-center text-sm text-muted-foreground">暂无数据</p>
      ) : (
        <div className="px-4 pb-3 pt-4">
          <div className="flex gap-2">
            <div className="flex h-48 w-14 shrink-0 flex-col justify-between text-right text-[10px] text-muted-foreground tabular">
              <span>{axis(max)}</span>
              <span>{axis(max / 2)}</span>
              <span>0</span>
            </div>
            <div className="relative flex h-48 flex-1 items-end gap-1.5 border-b border-l">
              {data.map((d) => {
                const v = valueOf(d);
                const pct = max > 0 ? (v / max) * 100 : 0;
                return (
                  <div
                    key={d.date}
                    className="group relative h-full flex-1"
                    title={[
                      d.date,
                      `请求 ${formatNumber(d.requests)}`,
                      `输入 ${formatNumber(d.inputTokens)} · 输出 ${formatNumber(d.outputTokens)}`,
                      `费用 ${formatMoney(d.costMicros, currency)}`,
                    ].join('\n')}
                  >
                    <div
                      className="absolute inset-x-0 bottom-0 rounded-t-sm bg-info/60 transition-colors group-hover:bg-info"
                      style={{ height: `${v > 0 ? Math.max(pct, 1.5) : 0}%` }}
                    />
                  </div>
                );
              })}
            </div>
          </div>
          <div className="ml-16 mt-1 flex gap-1.5">
            {data.map((d) => (
              <div key={d.date} className="flex-1 truncate text-center text-[10px] text-muted-foreground">
                {d.date.slice(5)}
              </div>
            ))}
          </div>
        </div>
      )}
    </Card>
  );
}

export function DashboardPage() {
  const { currency: settingsCurrency } = useSettings();
  const { data, error, loading, reload } = useAsync(() => adminApi.dashboard(), []);
  const currency = data?.currency || settingsCurrency;
  const totalTopRequests = (data?.topModels ?? []).reduce((s, m) => s + m.requests, 0);

  return (
    <div>
      <PageHeader
        title="仪表盘"
        description="今日数据按服务端时区统计；热门模型为近 24 小时。"
        actions={
          <Button size="sm" onClick={reload} loading={loading && !!data}>
            {loading && data ? null : <RefreshCw />}
            刷新
          </Button>
        }
      />
      {!data ? (
        error ? (
          <Card>
            <ErrorBlock error={error} onRetry={reload} />
          </Card>
        ) : (
          <LoadingBlock />
        )
      ) : (
        <div className="flex flex-col gap-4">
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-5">
            <StatCard
              icon={Users}
              label="用户"
              value={formatNumber(data.users.total)}
              sub={<span>7 日活跃 {formatNumber(data.users.active7d)}</span>}
            />
            <StatCard
              icon={KeyRound}
              label="上游账号"
              value={formatNumber(data.accounts.total)}
              sub={
                <>
                  <StatusDot tone="success">正常 {formatNumber(data.accounts.active)}</StatusDot>
                  <StatusDot tone="warning">冷却 {formatNumber(data.accounts.coolingDown)}</StatusDot>
                  <StatusDot tone="danger">异常 {formatNumber(data.accounts.error)}</StatusDot>
                </>
              }
            />
            <StatCard
              icon={Activity}
              label="今日请求"
              value={formatNumber(data.today.requests)}
              sub={
                <span className={data.today.errors > 0 ? 'text-danger' : undefined}>失败 {formatNumber(data.today.errors)}</span>
              }
            />
            <StatCard
              icon={Zap}
              label="今日 Token"
              value={formatCompact(data.today.inputTokens + data.today.outputTokens)}
              sub={
                <>
                  <span>输入 {formatNumber(data.today.inputTokens)}</span>
                  <span>输出 {formatNumber(data.today.outputTokens)}</span>
                </>
              }
            />
            <StatCard
              icon={Coins}
              label="今日费用"
              value={formatAmount(data.today.costMicros)}
              sub={<span>{currency}</span>}
            />
          </div>

          <DailyChart days={data.daily ?? []} currency={currency} />

          <Card>
            <CardHeader title="热门模型（24 小时）" />
            <Table>
              <THead>
                <tr>
                  <TH>模型</TH>
                  <TH className="text-right">请求数</TH>
                  <TH className="w-1/3">占比</TH>
                  <TH className="text-right">费用</TH>
                </tr>
              </THead>
              <TBody>
                {(data.topModels ?? []).length === 0 ? (
                  <tr>
                    <td colSpan={4} className="py-10 text-center text-sm text-muted-foreground">
                      暂无数据
                    </td>
                  </tr>
                ) : (
                  data.topModels.map((m) => {
                    const share = totalTopRequests > 0 ? (m.requests / totalTopRequests) * 100 : 0;
                    return (
                      <TR key={m.model}>
                        <TD className="font-mono text-xs">{m.model}</TD>
                        <TD className="tabular text-right">{formatNumber(m.requests)}</TD>
                        <TD>
                          <div className="flex items-center gap-2">
                            <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                              <div className="h-full rounded-full bg-info/70" style={{ width: `${share}%` }} />
                            </div>
                            <span className="tabular w-10 text-right text-xs text-muted-foreground">{share.toFixed(0)}%</span>
                          </div>
                        </TD>
                        <TD className="tabular text-right">{formatMoney(m.costMicros, currency)}</TD>
                      </TR>
                    );
                  })
                )}
              </TBody>
            </Table>
          </Card>
        </div>
      )}
    </div>
  );
}
