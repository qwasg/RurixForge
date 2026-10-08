import { ChevronDown, ChevronRight, RefreshCw, Search } from 'lucide-react';
import { Fragment, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/form';
import { Card, JsonBlock, PageHeader, Toolbar } from '@/components/ui/misc';
import { Pagination, Table, TableStatus, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { adminApi } from '@/lib/api/admin';
import { formatDateTime } from '@/lib/format';
import { useAsync, useDebounced, usePagination } from '@/lib/hooks';

function compact(detail: Record<string, unknown> | null): string {
  if (!detail || Object.keys(detail).length === 0) return '';
  try {
    const s = JSON.stringify(detail);
    return s.length > 120 ? `${s.slice(0, 120)}…` : s;
  } catch {
    return '';
  }
}

export function AuditLogsPage() {
  const [action, setAction] = useState('');
  const [actorId, setActorId] = useState('');
  const [q, setQ] = useState('');
  const actionQ = useDebounced(action.trim(), 300);
  const actorQ = useDebounced(actorId.trim(), 300);
  const query = useDebounced(q.trim(), 300);
  const page = usePagination(50);
  const list = useAsync(
    () =>
      adminApi.auditLogs.list({
        action: actionQ,
        actorId: /^\d+$/.test(actorQ) ? Number(actorQ) : undefined,
        q: query,
        limit: page.limit,
        offset: page.offset,
      }),
    [actionQ, actorQ, query, page.limit, page.offset],
  );
  const [expanded, setExpanded] = useState<number | null>(null);
  const items = list.data?.items ?? [];

  const filter = (setter: (v: string) => void) => (v: string) => {
    setter(v);
    page.reset();
  };

  return (
    <div>
      <PageHeader title="审计日志" description="所有管理写操作都会记录操作者、对象与详情。" />
      <Card>
        <Toolbar>
          <Input value={action} onChange={(e) => filter(setAction)(e.target.value)} placeholder="操作，如 account.create" aria-label="操作筛选" className="w-52 font-mono text-xs" />
          <Input value={actorId} onChange={(e) => filter(setActorId)(e.target.value)} placeholder="操作者 ID" aria-label="操作者 ID" inputMode="numeric" className="w-28" />
          <div className="relative w-52">
            <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input value={q} onChange={(e) => filter(setQ)(e.target.value)} placeholder="搜索对象 / 邮箱" aria-label="搜索审计日志" className="pl-7" />
          </div>
          <Button variant="ghost" size="sm" className="ml-auto" onClick={list.reload} loading={list.loading && !!list.data}>
            {list.loading && list.data ? null : <RefreshCw />}
            刷新
          </Button>
        </Toolbar>
        <Table>
          <THead>
            <tr>
              <TH className="w-8" />
              <TH>时间</TH>
              <TH>操作者</TH>
              <TH>操作</TH>
              <TH>对象</TH>
              <TH>详情</TH>
              <TH>IP</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus
              colSpan={7}
              loading={list.loading}
              error={list.error}
              empty={items.length === 0}
              emptyText="暂无审计记录"
              onRetry={list.reload}
            />
            {items.map((log) => {
              const open = expanded === log.id;
              const summary = compact(log.detail);
              return (
                <Fragment key={log.id}>
                  <TR className={summary ? 'cursor-pointer' : undefined} onClick={() => summary && setExpanded(open ? null : log.id)}>
                    <TD className="text-muted-foreground">
                      {summary ? open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" /> : null}
                    </TD>
                    <TD className="whitespace-nowrap text-xs">{formatDateTime(log.createdAt)}</TD>
                    <TD className="text-xs">
                      <div>{log.actorEmail || '系统'}</div>
                      {log.actorId != null ? <div className="text-[11px] text-muted-foreground">ID {log.actorId}</div> : null}
                    </TD>
                    <TD className="whitespace-nowrap font-mono text-xs">{log.action}</TD>
                    <TD className="max-w-48 truncate font-mono text-xs" title={log.target}>
                      {log.target || '—'}
                    </TD>
                    <TD className="max-w-md truncate font-mono text-[11px] text-muted-foreground">{summary || '—'}</TD>
                    <TD className="font-mono text-xs text-muted-foreground">{log.ip || '—'}</TD>
                  </TR>
                  {open ? (
                    <tr>
                      <td colSpan={7} className="border-b bg-muted/30 px-3 py-2">
                        <JsonBlock value={log.detail} />
                      </td>
                    </tr>
                  ) : null}
                </Fragment>
              );
            })}
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
