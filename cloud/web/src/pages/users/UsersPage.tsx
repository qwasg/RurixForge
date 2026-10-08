import { RefreshCw, Search, UserPlus } from 'lucide-react';
import { useState } from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input, Select } from '@/components/ui/form';
import { Card, PageHeader, Toolbar } from '@/components/ui/misc';
import { Pagination, Table, TableStatus, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { adminApi } from '@/lib/api/admin';
import type { UserStatus } from '@/lib/api/types';
import { formatDateTime } from '@/lib/format';
import { useAsync, useDebounced, usePagination } from '@/lib/hooks';
import { useGroups } from '@/lib/queries';
import { useMoney } from '@/lib/settings';
import { cn } from '@/lib/utils';
import { CreateUserDialog } from './CreateUserDialog';
import { ROLE_LABEL, ROLE_TONE, USER_STATUS_LABEL, USER_STATUS_TONE } from './labels';
import { UserDetailDrawer } from './UserDetailDrawer';

export function UsersPage() {
  const money = useMoney();
  const [q, setQ] = useState('');
  const query = useDebounced(q.trim(), 300);
  const [status, setStatus] = useState<UserStatus | ''>('');
  const page = usePagination(20);
  const list = useAsync(
    () => adminApi.users.list({ q: query, status, limit: page.limit, offset: page.offset }),
    [query, status, page.limit, page.offset],
  );
  const groups = useGroups();
  const [createOpen, setCreateOpen] = useState(false);
  const [detailId, setDetailId] = useState<number | null>(null);
  const items = list.data?.items ?? [];

  return (
    <div>
      <PageHeader
        title="用户"
        description="搜索、创建用户，调整余额、订阅与分组。"
        actions={
          <Button variant="primary" size="sm" onClick={() => setCreateOpen(true)}>
            <UserPlus />
            新建用户
          </Button>
        }
      />
      <Card>
        <Toolbar>
          <div className="relative w-64">
            <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={q}
              onChange={(e) => {
                setQ(e.target.value);
                page.reset();
              }}
              placeholder="搜索邮箱 / 昵称"
              aria-label="搜索用户"
              className="pl-7"
            />
          </div>
          <Select
            aria-label="状态筛选"
            className="w-32"
            value={status}
            onChange={(e) => {
              setStatus(e.target.value as UserStatus | '');
              page.reset();
            }}
          >
            <option value="">全部状态</option>
            <option value="active">正常</option>
            <option value="disabled">已禁用</option>
          </Select>
          <Button variant="ghost" size="sm" className="ml-auto" onClick={list.reload} loading={list.loading && !!list.data}>
            {list.loading && list.data ? null : <RefreshCw />}
            刷新
          </Button>
        </Toolbar>
        <Table>
          <THead>
            <tr>
              <TH>邮箱</TH>
              <TH>昵称</TH>
              <TH>角色</TH>
              <TH>状态</TH>
              <TH>分组</TH>
              <TH className="text-right">余额</TH>
              <TH>最近登录</TH>
              <TH>注册时间</TH>
              <TH className="text-right">操作</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus
              colSpan={9}
              loading={list.loading}
              error={list.error}
              empty={items.length === 0}
              emptyText={query || status ? '没有匹配的用户' : '暂无用户'}
              onRetry={list.reload}
            />
            {items.map((u) => (
              <TR key={u.id} className="cursor-pointer" onClick={() => setDetailId(u.id)}>
                <TD>
                  <div className="font-medium">{u.email}</div>
                  <div className="text-[11px] text-muted-foreground">ID {u.id}</div>
                </TD>
                <TD className="max-w-40 truncate">{u.nickname || '—'}</TD>
                <TD>
                  <Badge tone={ROLE_TONE[u.role]}>{ROLE_LABEL[u.role] ?? u.role}</Badge>
                </TD>
                <TD>
                  <Badge tone={USER_STATUS_TONE[u.status]}>{USER_STATUS_LABEL[u.status] ?? u.status}</Badge>
                </TD>
                <TD className="text-xs">{u.groupName || '默认'}</TD>
                <TD className={cn('tabular whitespace-nowrap text-right', u.balanceMicros < 0 && 'text-danger')}>{money(u.balanceMicros)}</TD>
                <TD className="whitespace-nowrap text-xs">{formatDateTime(u.lastLoginAt)}</TD>
                <TD className="whitespace-nowrap text-xs">{formatDateTime(u.createdAt)}</TD>
                <TD className="text-right">
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={(e) => {
                      e.stopPropagation();
                      setDetailId(u.id);
                    }}
                  >
                    详情
                  </Button>
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

      <CreateUserDialog open={createOpen} onOpenChange={setCreateOpen} groups={groups.data ?? []} onCreated={list.reload} />
      <UserDetailDrawer
        userId={detailId}
        onOpenChange={(open) => !open && setDetailId(null)}
        groups={groups.data ?? []}
        onChanged={list.reload}
      />
    </div>
  );
}
