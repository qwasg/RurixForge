import {
  Ellipsis,
  Eraser,
  Gauge,
  Info,
  Pencil,
  Play,
  Plus,
  Power,
  PowerOff,
  RefreshCw,
  RotateCcw,
  Search,
  Trash2,
} from 'lucide-react';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { useConfirm } from '@/components/ui/confirm';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown';
import { Input, Select } from '@/components/ui/form';
import { Card, Notice, PageHeader, Toolbar } from '@/components/ui/misc';
import { Pagination, Table, TableStatus, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { COMPLIANCE_HINT, cooldownRemaining, isAccount } from '@/lib/accounts';
import { adminApi } from '@/lib/api/admin';
import type { Account, AccountStatus, Platform } from '@/lib/api/types';
import { formatDateTime, formatRelative, secondsUntil } from '@/lib/format';
import { useAsync, useDebounced, usePagination } from '@/lib/hooks';
import { groupNameMap, useGroups, useModels } from '@/lib/queries';
import { useToast } from '@/lib/toast';
import { cn } from '@/lib/utils';
import { AccountDetailDrawer } from './AccountDetailDrawer';
import { AddAccountDialog } from './AddAccountDialog';
import { EditAccountDialog } from './EditAccountDialog';
import { AccountStatusBadge, CooldownCell, GroupNames, PlatformBadges, QuotaBars } from './shared';
import { TestAccountDialog } from './TestAccountDialog';

type RowAction = 'refresh' | 'quota' | 'cooldown' | 'toggle';

function TokenExpiry({ account }: { account: Account }) {
  if (account.authType !== 'oauth') {
    return account.keyHint ? <span className="font-mono text-xs text-muted-foreground">••••{account.keyHint}</span> : <span className="text-muted-foreground">—</span>;
  }
  const secs = secondsUntil(account.tokenExpiresAt);
  if (secs === null) return <span className="text-muted-foreground">—</span>;
  return (
    <span
      className={cn('whitespace-nowrap text-xs', secs <= 0 ? 'text-danger' : secs < 86400 ? 'text-warning' : undefined)}
      title={formatDateTime(account.tokenExpiresAt)}
    >
      {secs <= 0 ? '已过期' : formatRelative(account.tokenExpiresAt)}
    </span>
  );
}

export function AccountsPage() {
  const toast = useToast();
  const [q, setQ] = useState('');
  const query = useDebounced(q.trim(), 300);
  const [platform, setPlatform] = useState<Platform | ''>('');
  const [status, setStatus] = useState<AccountStatus | ''>('');
  const [groupId, setGroupId] = useState('');
  const page = usePagination(50);
  const list = useAsync(
    () =>
      adminApi.accounts.list({
        q: query,
        platform,
        status,
        groupId: groupId ? Number(groupId) : undefined,
        limit: page.limit,
        offset: page.offset,
      }),
    [query, platform, status, groupId, page.limit, page.offset],
  );
  const groups = useGroups();
  const models = useModels();
  const groupNames = groupNameMap(groups.data);
  const [confirmEl, confirm] = useConfirm();
  const [addOpen, setAddOpen] = useState(false);
  const [editing, setEditing] = useState<Account | null>(null);
  const [testing, setTesting] = useState<Account | null>(null);
  const [detailId, setDetailId] = useState<number | null>(null);
  const [busy, setBusy] = useState<{ id: number; action: RowAction } | null>(null);
  const items = list.data?.items ?? [];
  const detail = detailId === null ? null : items.find((a) => a.id === detailId) ?? null;

  const replaceRow = (res: unknown) => {
    if (!isAccount(res)) {
      list.reload();
      return;
    }
    list.setData((prev) => (prev ? { ...prev, items: prev.items.map((a) => (a.id === res.id ? res : a)) } : prev));
  };

  const run = async (account: Account, action: RowAction, fn: () => Promise<unknown>, success: string) => {
    setBusy({ id: account.id, action });
    try {
      replaceRow(await fn());
      toast.success(success);
    } catch (err) {
      toast.error(err);
    } finally {
      setBusy(null);
    }
  };

  const refreshQuota = (a: Account) => run(a, 'quota', () => adminApi.accounts.refreshQuota(a.id), `「${a.name}」额度已刷新`);

  const setFilter = <T,>(setter: (v: T) => void) => (v: T) => {
    setter(v);
    page.reset();
  };

  return (
    <div>
      <PageHeader
        title="上游账号"
        description="账号池：Codex 订阅（OAuth）与 API Key 账号，按分组、优先级、权重与并发调度。"
        actions={
          <Button variant="primary" size="sm" onClick={() => setAddOpen(true)}>
            <Plus />
            添加账号
          </Button>
        }
      />
      <Notice tone="warning" className="mb-3">
        {COMPLIANCE_HINT}
      </Notice>
      <Card>
        <Toolbar>
          <div className="relative w-60">
            <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={q}
              onChange={(e) => setFilter(setQ)(e.target.value)}
              placeholder="搜索名称 / 邮箱"
              aria-label="搜索账号"
              className="pl-7"
            />
          </div>
          <Select aria-label="平台筛选" className="w-32" value={platform} onChange={(e) => setFilter(setPlatform)(e.target.value as Platform | '')}>
            <option value="">全部平台</option>
            <option value="openai">OpenAI</option>
            <option value="anthropic">Anthropic</option>
          </Select>
          <Select aria-label="状态筛选" className="w-28" value={status} onChange={(e) => setFilter(setStatus)(e.target.value as AccountStatus | '')}>
            <option value="">全部状态</option>
            <option value="active">正常</option>
            <option value="disabled">已停用</option>
            <option value="error">异常</option>
          </Select>
          <Select aria-label="分组筛选" className="w-36" value={groupId} onChange={(e) => setFilter(setGroupId)(e.target.value)}>
            <option value="">全部分组</option>
            {(groups.data ?? []).map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </Select>
          <Button variant="ghost" size="sm" className="ml-auto" onClick={list.reload} loading={list.loading && !!list.data}>
            {list.loading && list.data ? null : <RefreshCw />}
            刷新
          </Button>
        </Toolbar>
        <Table>
          <THead>
            <tr>
              <TH>名称</TH>
              <TH>平台 / 认证</TH>
              <TH>邮箱 / 套餐</TH>
              <TH>状态</TH>
              <TH>冷却剩余</TH>
              <TH className="text-right">优先级</TH>
              <TH className="text-right">权重</TH>
              <TH className="text-right">并发</TH>
              <TH>分组</TH>
              <TH>最近错误</TH>
              <TH>Token 到期</TH>
              <TH>额度</TH>
              <TH className="text-right">操作</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus
              colSpan={13}
              loading={list.loading}
              error={list.error}
              empty={items.length === 0}
              emptyText={query || platform || status || groupId ? '没有匹配的账号' : '还没有上游账号，点右上角「添加账号」接入'}
              onRetry={list.reload}
            />
            {items.map((a) => {
              const rowBusy = busy?.id === a.id ? busy.action : null;
              const cooling = cooldownRemaining(a, Date.now()) > 0;
              return (
                <TR key={a.id}>
                  <TD>
                    <button type="button" className="text-left font-medium hover:underline" onClick={() => setDetailId(a.id)}>
                      {a.name}
                    </button>
                    <div className="text-[11px] text-muted-foreground">ID {a.id}</div>
                  </TD>
                  <TD>
                    <PlatformBadges account={a} />
                  </TD>
                  <TD className="max-w-52">
                    {a.authType === 'oauth' ? (
                      <>
                        <div className="truncate text-xs" title={a.email}>
                          {a.email || '—'}
                        </div>
                        <div className="text-[11px] text-muted-foreground">{a.planType || '—'}</div>
                      </>
                    ) : (
                      <div className="truncate font-mono text-[11px] text-muted-foreground" title={a.baseUrl}>
                        {a.baseUrl || '—'}
                      </div>
                    )}
                  </TD>
                  <TD>
                    <AccountStatusBadge account={a} />
                  </TD>
                  <TD>
                    <CooldownCell account={a} />
                  </TD>
                  <TD className="tabular text-right">{a.priority}</TD>
                  <TD className="tabular text-right">{a.weight}</TD>
                  <TD className="tabular whitespace-nowrap text-right">
                    <span className={a.concurrencyLimit > 0 && a.currentConcurrency >= a.concurrencyLimit ? 'text-warning' : undefined}>
                      {a.currentConcurrency}
                    </span>
                    <span className="text-muted-foreground"> / {a.concurrencyLimit}</span>
                  </TD>
                  <TD>
                    <GroupNames ids={a.groupIds} names={groupNames} />
                  </TD>
                  <TD className="max-w-56">
                    {a.lastError ? (
                      <>
                        <div className="truncate text-xs text-danger" title={a.lastError}>
                          {a.lastError}
                        </div>
                        <div className="text-[11px] text-muted-foreground" title={formatDateTime(a.lastErrorAt)}>
                          {formatRelative(a.lastErrorAt)}
                        </div>
                      </>
                    ) : (
                      <span className="text-muted-foreground">—</span>
                    )}
                  </TD>
                  <TD>
                    <TokenExpiry account={a} />
                  </TD>
                  <TD>
                    <QuotaBars quota={a.quota} />
                  </TD>
                  <TD>
                    <div className="flex items-center justify-end gap-1">
                      <Button size="sm" variant="ghost" onClick={() => setTesting(a)}>
                        <Play />
                        测试
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => setEditing(a)}>
                        <Pencil />
                        编辑
                      </Button>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button size="icon-sm" variant="ghost" aria-label={`${a.name} 更多操作`} loading={rowBusy !== null}>
                            {rowBusy !== null ? null : <Ellipsis />}
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent>
                          <DropdownMenuItem onSelect={() => setDetailId(a.id)}>
                            <Info />
                            详情
                          </DropdownMenuItem>
                          {a.authType === 'oauth' ? (
                            <DropdownMenuItem
                              onSelect={() => run(a, 'refresh', () => adminApi.accounts.refreshToken(a.id), `「${a.name}」token 已刷新`)}
                            >
                              <RotateCcw />
                              刷新 token
                            </DropdownMenuItem>
                          ) : null}
                          <DropdownMenuItem disabled={a.authType !== 'oauth'} onSelect={() => refreshQuota(a)}>
                            <Gauge />
                            刷新额度
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            disabled={!cooling}
                            onSelect={() => run(a, 'cooldown', () => adminApi.accounts.clearCooldown(a.id), `「${a.name}」冷却已清除`)}
                          >
                            <Eraser />
                            清除冷却
                          </DropdownMenuItem>
                          {a.status === 'disabled' || a.status === 'error' ? (
                            <DropdownMenuItem
                              onSelect={() =>
                                run(a, 'toggle', () => adminApi.accounts.update(a.id, { status: 'active' }), `「${a.name}」已启用`)
                              }
                            >
                              <Power />
                              启用
                            </DropdownMenuItem>
                          ) : (
                            <DropdownMenuItem
                              onSelect={() =>
                                run(a, 'toggle', () => adminApi.accounts.update(a.id, { status: 'disabled' }), `「${a.name}」已停用`)
                              }
                            >
                              <PowerOff />
                              停用
                            </DropdownMenuItem>
                          )}
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            danger
                            onSelect={() =>
                              confirm({
                                title: `删除账号「${a.name}」`,
                                description: '账号及其加密凭据将被永久删除，不可恢复。只想暂停调度可改为「停用」。',
                                confirmText: '删除',
                                danger: true,
                                action: async () => {
                                  await adminApi.accounts.remove(a.id);
                                  toast.success('账号已删除');
                                  list.reload();
                                },
                              })
                            }
                          >
                            <Trash2 />
                            删除
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </div>
                  </TD>
                </TR>
              );
            })}
          </TBody>
        </Table>
        <Pagination
          total={list.data?.total ?? items.length}
          limit={page.limit}
          offset={page.offset}
          onOffsetChange={page.setOffset}
          onLimitChange={page.setLimit}
        />
      </Card>

      <AddAccountDialog open={addOpen} onOpenChange={setAddOpen} groups={groups.data ?? []} accounts={items} onChanged={list.reload} />
      <EditAccountDialog
        account={editing}
        onOpenChange={(o) => !o && setEditing(null)}
        groups={groups.data ?? []}
        onSaved={replaceRow}
      />
      <TestAccountDialog account={testing} onOpenChange={(o) => !o && setTesting(null)} models={models.data ?? []} />
      <AccountDetailDrawer
        account={detail}
        onOpenChange={(o) => !o && setDetailId(null)}
        groupNames={groupNames}
        onRefreshQuota={refreshQuota}
        refreshingQuota={busy?.action === 'quota' && busy.id === detailId}
      />
      {confirmEl}
    </div>
  );
}
