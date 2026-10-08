import { Ban, Download, Plus, RefreshCw, Search } from 'lucide-react';
import { useId, useState, type FormEvent } from 'react';
import { Badge, type BadgeTone } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { useConfirm } from '@/components/ui/confirm';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Field, Input, Select, Textarea } from '@/components/ui/form';
import { Card, CopyButton, FormError, PageHeader, Toolbar } from '@/components/ui/misc';
import { Pagination, Table, TableStatus, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { CreateRedeemInput, CreateRedeemResult, Plan, RedeemKind, RedeemStatus } from '@/lib/api/types';
import { saveBlob } from '@/lib/browser';
import { dateTimeLocalToRfc3339, formatDateTime, parseIntInput, unitsToMicros } from '@/lib/format';
import { useAsync, useDebounced, usePagination } from '@/lib/hooks';
import { usePlans } from '@/lib/queries';
import { useMoney, useSettings } from '@/lib/settings';
import { useToast } from '@/lib/toast';

const KIND_LABEL: Record<RedeemKind, string> = { balance: '余额', plan: '套餐', invite: '邀请' };
const KIND_TONE: Record<RedeemKind, BadgeTone> = { balance: 'success', plan: 'info', invite: 'neutral' };
const STATUS_LABEL: Record<RedeemStatus, string> = { active: '可用', revoked: '已作废', exhausted: '已用尽', expired: '已过期' };
const STATUS_TONE: Record<RedeemStatus, BadgeTone> = { active: 'success', revoked: 'danger', exhausted: 'neutral', expired: 'warning' };

function useExportCsv() {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const exportCsv = async (batch: string) => {
    setBusy(true);
    try {
      const { blob, filename } = await adminApi.redeemCodes.exportCsv(batch || undefined);
      saveBlob(blob, filename || `redeem-codes-${batch || 'all'}.csv`);
    } catch (err) {
      toast.error(err);
    } finally {
      setBusy(false);
    }
  };
  return { exportCsv, exporting: busy };
}

export function RedeemCodesPage() {
  const money = useMoney();
  const toast = useToast();
  const [confirmEl, confirm] = useConfirm();
  const [batch, setBatch] = useState('');
  const [status, setStatus] = useState<RedeemStatus | ''>('');
  const [kind, setKind] = useState<RedeemKind | ''>('');
  const [q, setQ] = useState('');
  const batchQ = useDebounced(batch.trim(), 300);
  const query = useDebounced(q.trim(), 300);
  const page = usePagination(50);
  const list = useAsync(
    () => adminApi.redeemCodes.list({ batch: batchQ, status, kind, q: query, limit: page.limit, offset: page.offset }),
    [batchQ, status, kind, query, page.limit, page.offset],
  );
  const plans = usePlans();
  const { exportCsv, exporting } = useExportCsv();
  const [createOpen, setCreateOpen] = useState(false);
  const items = list.data?.items ?? [];

  return (
    <div>
      <PageHeader
        title="兑换码"
        description="余额码加余额，套餐码开通订阅，邀请码用于邀请制注册；每人每码限用一次。"
        actions={
          <>
            <Button size="sm" onClick={() => exportCsv(batchQ)} loading={exporting}>
              {exporting ? null : <Download />}
              {batchQ ? `导出批次 ${batchQ}` : '导出 CSV'}
            </Button>
            <Button size="sm" variant="primary" onClick={() => setCreateOpen(true)}>
              <Plus />
              生成兑换码
            </Button>
          </>
        }
      />
      <Card>
        <Toolbar>
          <Input
            value={batch}
            onChange={(e) => {
              setBatch(e.target.value);
              page.reset();
            }}
            placeholder="批次"
            aria-label="批次筛选"
            className="w-40"
          />
          <Select
            aria-label="类型筛选"
            className="w-28"
            value={kind}
            onChange={(e) => {
              setKind(e.target.value as RedeemKind | '');
              page.reset();
            }}
          >
            <option value="">全部类型</option>
            <option value="balance">余额</option>
            <option value="plan">套餐</option>
            <option value="invite">邀请</option>
          </Select>
          <Select
            aria-label="状态筛选"
            className="w-28"
            value={status}
            onChange={(e) => {
              setStatus(e.target.value as RedeemStatus | '');
              page.reset();
            }}
          >
            <option value="">全部状态</option>
            <option value="active">可用</option>
            <option value="exhausted">已用尽</option>
            <option value="expired">已过期</option>
            <option value="revoked">已作废</option>
          </Select>
          <div className="relative w-52">
            <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={q}
              onChange={(e) => {
                setQ(e.target.value);
                page.reset();
              }}
              placeholder="搜索兑换码 / 备注"
              aria-label="搜索兑换码"
              className="pl-7"
            />
          </div>
          <Button variant="ghost" size="sm" className="ml-auto" onClick={list.reload} loading={list.loading && !!list.data}>
            {list.loading && list.data ? null : <RefreshCw />}
            刷新
          </Button>
        </Toolbar>
        <Table>
          <THead>
            <tr>
              <TH>兑换码</TH>
              <TH>类型</TH>
              <TH>面值 / 套餐</TH>
              <TH>批次</TH>
              <TH className="text-right">已用 / 次数</TH>
              <TH>状态</TH>
              <TH>过期时间</TH>
              <TH>备注</TH>
              <TH>创建时间</TH>
              <TH className="text-right">操作</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus
              colSpan={10}
              loading={list.loading}
              error={list.error}
              empty={items.length === 0}
              emptyText="暂无兑换码"
              onRetry={list.reload}
            />
            {items.map((c) => (
              <TR key={c.id}>
                <TD>
                  <div className="flex items-center gap-1">
                    <span className="font-mono text-xs">{c.code}</span>
                    <CopyButton text={c.code} iconOnly label={`复制 ${c.code}`} />
                  </div>
                </TD>
                <TD>
                  <Badge tone={KIND_TONE[c.kind]}>{KIND_LABEL[c.kind] ?? c.kind}</Badge>
                </TD>
                <TD className="whitespace-nowrap text-xs">
                  {c.kind === 'balance' ? money(c.valueMicros) : c.kind === 'plan' ? c.planName || `套餐 #${c.planId}` : '—'}
                </TD>
                <TD>
                  {c.batch ? (
                    <button
                      type="button"
                      className="font-mono text-xs hover:underline"
                      title="按此批次筛选"
                      onClick={() => {
                        setBatch(c.batch);
                        page.reset();
                      }}
                    >
                      {c.batch}
                    </button>
                  ) : (
                    '—'
                  )}
                </TD>
                <TD className="tabular text-right">
                  {c.usedCount} / {c.maxUses}
                </TD>
                <TD>
                  <Badge tone={STATUS_TONE[c.status]}>{STATUS_LABEL[c.status] ?? c.status}</Badge>
                </TD>
                <TD className="whitespace-nowrap text-xs">{c.expiresAt ? formatDateTime(c.expiresAt) : '永不'}</TD>
                <TD className="max-w-40 truncate text-xs text-muted-foreground" title={c.note}>
                  {c.note || '—'}
                </TD>
                <TD className="whitespace-nowrap text-xs">{formatDateTime(c.createdAt)}</TD>
                <TD className="text-right">
                  {c.status === 'active' ? (
                    <Button
                      size="sm"
                      variant="danger-ghost"
                      onClick={() =>
                        confirm({
                          title: `作废兑换码 ${c.code}`,
                          description: '作废后该码无法再兑换，已兑换的余额/订阅不受影响。',
                          confirmText: '作废',
                          danger: true,
                          action: async () => {
                            await adminApi.redeemCodes.revoke(c.id);
                            toast.success('已作废');
                            list.reload();
                          },
                        })
                      }
                    >
                      <Ban />
                      作废
                    </Button>
                  ) : null}
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

      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        {createOpen ? (
          <GenerateForm
            plans={plans.data ?? []}
            onClose={() => setCreateOpen(false)}
            onCreated={(res) => {
              setBatch(res.batch);
              setStatus('');
              setKind('');
              setQ('');
              page.reset();
              list.reload();
            }}
          />
        ) : null}
      </Dialog>
      {confirmEl}
    </div>
  );
}

function GenerateForm({
  plans,
  onClose,
  onCreated,
}: {
  plans: Plan[];
  onClose: () => void;
  onCreated: (res: CreateRedeemResult) => void;
}) {
  const formId = useId();
  const money = useMoney();
  const { currency } = useSettings();
  const { exportCsv, exporting } = useExportCsv();
  const [kind, setKind] = useState<RedeemKind>('balance');
  const [value, setValue] = useState('10');
  const [planId, setPlanId] = useState('');
  const [count, setCount] = useState('10');
  const [maxUses, setMaxUses] = useState('1');
  const [expiresAt, setExpiresAt] = useState('');
  const [note, setNote] = useState('');
  const [batch, setBatch] = useState('');
  const [prefix, setPrefix] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<CreateRedeemResult | null>(null);

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const valueMicros = kind === 'balance' ? unitsToMicros(value) : 0;
    if (kind === 'balance' && (Number.isNaN(valueMicros) || valueMicros <= 0)) return setError('面值应为大于 0 的数字');
    if (kind === 'plan' && !planId) return setError('请选择套餐');
    const n = parseIntInput(count);
    if (Number.isNaN(n) || n < 1 || n > 1000) return setError('数量应为 1–1000 的整数');
    const uses = parseIntInput(maxUses, 1);
    if (Number.isNaN(uses) || uses < 1) return setError('每码可用次数应为正整数');
    const expires = expiresAt ? dateTimeLocalToRfc3339(expiresAt) : null;
    if (expiresAt && !expires) return setError('过期时间格式不正确');
    if (prefix && !/^[A-Za-z0-9_-]{0,16}$/.test(prefix)) return setError('前缀只能包含字母、数字、- 和 _，最长 16 位');
    const input: CreateRedeemInput = {
      kind,
      valueMicros,
      planId: kind === 'plan' ? Number(planId) : null,
      count: n,
      maxUses: uses,
      expiresAt: expires ?? null,
      note: note.trim(),
      batch: batch.trim(),
      prefix: prefix.trim(),
    };
    setError(null);
    setBusy(true);
    try {
      const res = await adminApi.redeemCodes.create(input);
      setResult(res);
      onCreated(res);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  if (result) {
    const codes = (result.items ?? []).map((c) => c.code).join('\n');
    return (
      <DialogContent
        title="兑换码已生成"
        description={`批次 ${result.batch} · 共 ${result.items?.length ?? 0} 个`}
        footer={
          <>
            <Button onClick={() => exportCsv(result.batch)} loading={exporting}>
              {exporting ? null : <Download />}
              导出 CSV
            </Button>
            <CopyButton text={codes} label="复制全部" />
            <Button variant="primary" onClick={onClose}>
              完成
            </Button>
          </>
        }
      >
        <Textarea readOnly value={codes} aria-label="生成的兑换码" className="min-h-64 font-mono text-xs" onFocus={(e) => e.currentTarget.select()} />
      </DialogContent>
    );
  }

  return (
    <DialogContent
      title="生成兑换码"
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button type="submit" form={formId} variant="primary" loading={busy}>
            生成
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={onSubmit} className="grid grid-cols-2 gap-3" noValidate>
        <Field label="类型">
          <Select value={kind} onChange={(e) => setKind(e.target.value as RedeemKind)}>
            <option value="balance">余额</option>
            <option value="plan">套餐</option>
            <option value="invite">邀请（注册用）</option>
          </Select>
        </Field>
        {kind === 'balance' ? (
          <Field label={`面值（${currency}）`} required>
            <Input inputMode="decimal" value={value} onChange={(e) => setValue(e.target.value)} />
          </Field>
        ) : kind === 'plan' ? (
          <Field label="套餐" required>
            <Select value={planId} onChange={(e) => setPlanId(e.target.value)}>
              <option value="">选择套餐…</option>
              {plans.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name} · {money(p.quotaMicros)} / {p.periodDays} 天{p.enabled ? '' : '（已停用）'}
                </option>
              ))}
            </Select>
          </Field>
        ) : (
          <div />
        )}
        <Field label="数量" hint="1–1000">
          <Input inputMode="numeric" value={count} onChange={(e) => setCount(e.target.value)} />
        </Field>
        <Field label="每码可用次数" hint="每个用户每码只能兑换一次">
          <Input inputMode="numeric" value={maxUses} onChange={(e) => setMaxUses(e.target.value)} />
        </Field>
        <Field label="过期时间（可选）">
          <Input type="datetime-local" value={expiresAt} onChange={(e) => setExpiresAt(e.target.value)} />
        </Field>
        <Field label="批次（可选）" hint="留空由服务端生成">
          <Input value={batch} onChange={(e) => setBatch(e.target.value)} className="font-mono text-xs" />
        </Field>
        <Field label="前缀（可选）" hint="兑换码前缀，如 VIP">
          <Input value={prefix} onChange={(e) => setPrefix(e.target.value)} className="font-mono text-xs" />
        </Field>
        <Field label="备注（可选）">
          <Input value={note} onChange={(e) => setNote(e.target.value)} />
        </Field>
        <div className="col-span-2">
          <FormError>{error}</FormError>
        </div>
      </form>
    </DialogContent>
  );
}
