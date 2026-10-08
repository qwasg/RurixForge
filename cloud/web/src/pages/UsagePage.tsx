import { RefreshCw, RotateCcw, Search } from 'lucide-react';
import { useState, type FormEvent } from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Field, Input, Select } from '@/components/ui/form';
import { Card, PageHeader } from '@/components/ui/misc';
import { Pagination, Table, TableStatus, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { adminApi } from '@/lib/api/admin';
import type { UsageListParams } from '@/lib/api/types';
import { dateInputToRfc3339, formatAmount, formatDateTime, formatNumber } from '@/lib/format';
import { useAsync, usePagination } from '@/lib/hooks';
import { useModels } from '@/lib/queries';
import { useMoney, useSettings } from '@/lib/settings';

interface Filters {
  userId: string;
  accountId: string;
  model: string;
  status: '' | 'ok' | 'error';
  from: string;
  to: string;
}

const EMPTY: Filters = { userId: '', accountId: '', model: '', status: '', from: '', to: '' };

function toParams(f: Filters): UsageListParams {
  const id = (v: string) => (/^\d+$/.test(v.trim()) ? Number(v.trim()) : undefined);
  return {
    userId: id(f.userId),
    accountId: id(f.accountId),
    model: f.model.trim() || undefined,
    status: f.status,
    from: f.from ? dateInputToRfc3339(f.from) : undefined,
    to: f.to ? dateInputToRfc3339(f.to, true) : undefined,
  };
}

function Summary({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <Card className="px-4 py-3">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="tabular mt-1 text-lg font-semibold">{value}</div>
      {sub ? <div className="text-xs text-muted-foreground">{sub}</div> : null}
    </Card>
  );
}

export function UsagePage() {
  const money = useMoney();
  const { currency } = useSettings();
  const models = useModels();
  const [draft, setDraft] = useState<Filters>(EMPTY);
  const [applied, setApplied] = useState<Filters>(EMPTY);
  const page = usePagination(50);
  const list = useAsync(
    () => adminApi.usage.list({ ...toParams(applied), limit: page.limit, offset: page.offset }),
    [applied, page.limit, page.offset],
  );
  const items = list.data?.items ?? [];
  const s = list.data?.summary;
  const set = (patch: Partial<Filters>) => setDraft((d) => ({ ...d, ...patch }));

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setApplied({ ...draft });
    page.reset();
  };

  return (
    <div>
      <PageHeader title="用量" description="每次网关请求一条记录；失败请求费用为 0。" />
      <Card className="mb-3">
        <form onSubmit={onSubmit} className="flex flex-wrap items-end gap-2 p-3" noValidate>
          <Field label="用户 ID" className="w-24">
            <Input inputMode="numeric" value={draft.userId} onChange={(e) => set({ userId: e.target.value })} />
          </Field>
          <Field label="账号 ID" className="w-24">
            <Input inputMode="numeric" value={draft.accountId} onChange={(e) => set({ accountId: e.target.value })} />
          </Field>
          <Field label="模型" className="w-44">
            <Input list="usage-models" value={draft.model} onChange={(e) => set({ model: e.target.value })} className="font-mono text-xs" />
          </Field>
          <datalist id="usage-models">
            {(models.data ?? []).map((m) => (
              <option key={m.id} value={m.id} />
            ))}
          </datalist>
          <Field label="状态" className="w-24">
            <Select value={draft.status} onChange={(e) => set({ status: e.target.value as Filters['status'] })}>
              <option value="">全部</option>
              <option value="ok">成功</option>
              <option value="error">失败</option>
            </Select>
          </Field>
          <Field label="开始日期" className="w-36">
            <Input type="date" value={draft.from} onChange={(e) => set({ from: e.target.value })} />
          </Field>
          <Field label="结束日期" className="w-36">
            <Input type="date" value={draft.to} onChange={(e) => set({ to: e.target.value })} />
          </Field>
          <Button type="submit" variant="primary">
            <Search />
            查询
          </Button>
          <Button
            onClick={() => {
              setDraft(EMPTY);
              setApplied(EMPTY);
              page.reset();
            }}
          >
            <RotateCcw />
            重置
          </Button>
          <Button variant="ghost" className="ml-auto" onClick={list.reload} loading={list.loading && !!list.data}>
            {list.loading && list.data ? null : <RefreshCw />}
            刷新
          </Button>
        </form>
      </Card>

      <div className="mb-3 grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
        <Summary label="请求数" value={formatNumber(s?.requests)} />
        <Summary label="输入 Token" value={formatNumber(s?.inputTokens)} />
        <Summary label="输出 Token" value={formatNumber(s?.outputTokens)} />
        <Summary label="缓存读 Token" value={formatNumber(s?.cacheReadTokens)} />
        <Summary label="缓存写 Token" value={formatNumber(s?.cacheWriteTokens)} />
        <Summary label="费用" value={formatAmount(s?.costMicros)} sub={currency} />
      </div>

      <Card>
        <Table>
          <THead>
            <tr>
              <TH>时间</TH>
              <TH>用户</TH>
              <TH>模型</TH>
              <TH>账号</TH>
              <TH>端点</TH>
              <TH className="text-right">输入</TH>
              <TH className="text-right">输出</TH>
              <TH className="text-right">缓存 读/写</TH>
              <TH className="text-right">费用</TH>
              <TH>状态</TH>
              <TH className="text-right">首字 / 总耗时</TH>
              <TH>IP</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus
              colSpan={12}
              loading={list.loading}
              error={list.error}
              empty={items.length === 0}
              emptyText="没有符合条件的记录"
              onRetry={list.reload}
            />
            {items.map((u) => (
              <TR key={u.id}>
                <TD className="whitespace-nowrap text-xs" title={`请求 ID：${u.requestId}`}>
                  {formatDateTime(u.createdAt)}
                </TD>
                <TD className="max-w-44">
                  <div className="truncate text-xs" title={u.userEmail}>
                    {u.userEmail || '—'}
                  </div>
                  <div className="text-[11px] text-muted-foreground">
                    ID {u.userId}
                    {u.apiKeyName ? ` · ${u.apiKeyName}` : ''}
                  </div>
                </TD>
                <TD>
                  <div className="font-mono text-xs">{u.model}</div>
                  {u.upstreamModel && u.upstreamModel !== u.model ? (
                    <div className="font-mono text-[11px] text-muted-foreground">→ {u.upstreamModel}</div>
                  ) : null}
                </TD>
                <TD className="max-w-36 truncate text-xs" title={u.accountName}>
                  {u.accountName || (u.accountId != null ? `#${u.accountId}` : '—')}
                </TD>
                <TD>
                  <div className="flex items-center gap-1">
                    <Badge tone="outline">{u.endpoint}</Badge>
                    {u.stream ? <Badge tone="neutral">流式</Badge> : null}
                  </div>
                </TD>
                <TD className="tabular text-right">{formatNumber(u.inputTokens)}</TD>
                <TD className="tabular text-right">{formatNumber(u.outputTokens)}</TD>
                <TD className="tabular whitespace-nowrap text-right text-xs text-muted-foreground">
                  {formatNumber(u.cacheReadTokens)} / {formatNumber(u.cacheWriteTokens)}
                </TD>
                <TD className="tabular whitespace-nowrap text-right">{money(u.costMicros)}</TD>
                <TD>
                  {u.status === 'ok' ? (
                    <Badge tone="success">成功</Badge>
                  ) : (
                    <div className="flex flex-col items-start gap-0.5">
                      <Badge tone="danger">失败{u.httpStatus ? ` ${u.httpStatus}` : ''}</Badge>
                      {u.errorCode ? <span className="font-mono text-[11px] text-muted-foreground">{u.errorCode}</span> : null}
                    </div>
                  )}
                </TD>
                <TD className="tabular whitespace-nowrap text-right text-xs">
                  {u.firstTokenMs ? `${formatNumber(u.firstTokenMs)} / ` : ''}
                  {formatNumber(u.latencyMs)} ms
                </TD>
                <TD className="font-mono text-xs text-muted-foreground">{u.ip || '—'}</TD>
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
    </div>
  );
}
