import { Pencil, Plus, RefreshCw, Trash2 } from 'lucide-react';
import { useId, useState, type FormEvent } from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { useConfirm } from '@/components/ui/confirm';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Checkbox, CheckboxGroup, Field, Input, Textarea } from '@/components/ui/form';
import { Card, FormError, PageHeader } from '@/components/ui/misc';
import { Table, TableStatus, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { AdminModel, Group, GroupInput } from '@/lib/api/types';
import { formatNumber, parseIntInput } from '@/lib/format';
import { useGroups, useModels } from '@/lib/queries';
import { useToast } from '@/lib/toast';

function limitText(n: number): string {
  return n > 0 ? formatNumber(n) : '不限';
}

export function GroupsPage() {
  const groups = useGroups();
  const models = useModels();
  const toast = useToast();
  const [confirmEl, confirm] = useConfirm();
  const [editing, setEditing] = useState<Group | 'new' | null>(null);
  const items = groups.data ?? [];

  return (
    <div>
      <PageHeader
        title="分组"
        description="分组决定计费倍率、每用户并发、RPM/TPM 限流与可用模型；上游账号按分组调度。"
        actions={
          <>
            <Button size="sm" variant="ghost" onClick={groups.reload}>
              <RefreshCw />
              刷新
            </Button>
            <Button size="sm" variant="primary" onClick={() => setEditing('new')}>
              <Plus />
              新建分组
            </Button>
          </>
        }
      />
      <Card>
        <Table>
          <THead>
            <tr>
              <TH>名称</TH>
              <TH>描述</TH>
              <TH className="text-right">倍率</TH>
              <TH className="text-right">每用户并发</TH>
              <TH className="text-right">RPM</TH>
              <TH className="text-right">TPM</TH>
              <TH>可用模型</TH>
              <TH className="text-right">用户</TH>
              <TH className="text-right">账号</TH>
              <TH className="text-right">操作</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus
              colSpan={10}
              loading={groups.loading}
              error={groups.error}
              empty={items.length === 0}
              emptyText="暂无分组"
              onRetry={groups.reload}
            />
            {items.map((g) => (
              <TR key={g.id}>
                <TD>
                  <div className="flex items-center gap-1.5 font-medium">
                    {g.name}
                    {g.isDefault ? <Badge tone="info">默认</Badge> : null}
                  </div>
                  <div className="text-[11px] text-muted-foreground">ID {g.id}</div>
                </TD>
                <TD className="max-w-56 truncate text-xs text-muted-foreground" title={g.description}>
                  {g.description || '—'}
                </TD>
                <TD className="tabular text-right">×{g.rateMultiplier}</TD>
                <TD className="tabular text-right">{limitText(g.concurrencyLimit)}</TD>
                <TD className="tabular text-right">{limitText(g.rpmLimit)}</TD>
                <TD className="tabular text-right">{limitText(g.tpmLimit)}</TD>
                <TD className="max-w-64 text-xs">
                  {(g.allowedModels ?? []).length === 0 ? (
                    <span className="text-muted-foreground">全部</span>
                  ) : (
                    <span title={g.allowedModels.join('\n')}>
                      {g.allowedModels.slice(0, 3).join('、')}
                      {g.allowedModels.length > 3 ? ` 等 ${g.allowedModels.length} 个` : ''}
                    </span>
                  )}
                </TD>
                <TD className="tabular text-right">{formatNumber(g.userCount)}</TD>
                <TD className="tabular text-right">{formatNumber(g.accountCount)}</TD>
                <TD>
                  <div className="flex justify-end gap-1">
                    <Button size="icon-sm" variant="ghost" aria-label={`编辑 ${g.name}`} onClick={() => setEditing(g)}>
                      <Pencil />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="danger-ghost"
                      aria-label={`删除 ${g.name}`}
                      onClick={() =>
                        confirm({
                          title: `删除分组「${g.name}」`,
                          description: '被用户或套餐引用的分组无法删除（服务端返回 GROUP_IN_USE）。',
                          confirmText: '删除',
                          danger: true,
                          action: async () => {
                            await adminApi.groups.remove(g.id);
                            toast.success('分组已删除');
                            groups.reload();
                          },
                        })
                      }
                    >
                      <Trash2 />
                    </Button>
                  </div>
                </TD>
              </TR>
            ))}
          </TBody>
        </Table>
      </Card>

      <Dialog open={editing !== null} onOpenChange={(o) => !o && setEditing(null)}>
        {editing !== null ? (
          <GroupForm
            group={editing === 'new' ? null : editing}
            models={models.data ?? []}
            onClose={() => setEditing(null)}
            onSaved={groups.reload}
          />
        ) : null}
      </Dialog>
      {confirmEl}
    </div>
  );
}

function GroupForm({
  group,
  models,
  onClose,
  onSaved,
}: {
  group: Group | null;
  models: AdminModel[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const formId = useId();
  const toast = useToast();
  const [name, setName] = useState(group?.name ?? '');
  const [description, setDescription] = useState(group?.description ?? '');
  const [rate, setRate] = useState(String(group?.rateMultiplier ?? 1));
  const [concurrency, setConcurrency] = useState(String(group?.concurrencyLimit ?? 5));
  const [rpm, setRpm] = useState(String(group?.rpmLimit ?? 0));
  const [tpm, setTpm] = useState(String(group?.tpmLimit ?? 0));
  const [allowed, setAllowed] = useState<string[]>(group?.allowedModels ?? []);
  const [isDefault, setIsDefault] = useState(group?.isDefault ?? false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // 已删除的模型仍在 allowedModels 里时也要能看到、取消勾选。
  const modelIds = [...new Set([...models.map((m) => m.id), ...allowed])];
  const displayName = new Map(models.map((m) => [m.id, m.displayName]));

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const rateMultiplier = Number(rate);
    const concurrencyLimit = parseIntInput(concurrency);
    const rpmLimit = parseIntInput(rpm);
    const tpmLimit = parseIntInput(tpm);
    if (!name.trim()) return setError('请填写分组名称');
    if (!Number.isFinite(rateMultiplier) || rateMultiplier <= 0 || rate.trim() === '') return setError('倍率应为大于 0 的数字');
    for (const [v, label] of [
      [concurrencyLimit, '每用户并发'],
      [rpmLimit, 'RPM'],
      [tpmLimit, 'TPM'],
    ] as const) {
      if (Number.isNaN(v) || v < 0) return setError(`${label}应为非负整数（0 = 不限）`);
    }
    const input: GroupInput = {
      name: name.trim(),
      description: description.trim(),
      rateMultiplier,
      concurrencyLimit,
      rpmLimit,
      tpmLimit,
      allowedModels: allowed,
      isDefault,
    };
    setError(null);
    setBusy(true);
    try {
      if (group) await adminApi.groups.update(group.id, input);
      else await adminApi.groups.create(input);
      toast.success(group ? '分组已保存' : '分组已创建');
      onSaved();
      onClose();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogContent
      title={group ? `编辑分组「${group.name}」` : '新建分组'}
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button type="submit" form={formId} variant="primary" loading={busy}>
            保存
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={onSubmit} className="grid grid-cols-2 gap-3" noValidate>
        <Field label="名称" required>
          <Input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        </Field>
        <Field label="计费倍率" hint="费用 = 模型单价 × 倍率">
          <Input inputMode="decimal" value={rate} onChange={(e) => setRate(e.target.value)} />
        </Field>
        <Field label="描述" className="col-span-2">
          <Textarea value={description} onChange={(e) => setDescription(e.target.value)} className="min-h-14" />
        </Field>
        <Field label="每用户并发" hint="0 = 不限">
          <Input inputMode="numeric" value={concurrency} onChange={(e) => setConcurrency(e.target.value)} />
        </Field>
        <Field label="RPM（每分钟请求）" hint="0 = 不限">
          <Input inputMode="numeric" value={rpm} onChange={(e) => setRpm(e.target.value)} />
        </Field>
        <Field label="TPM（每分钟 Token）" hint="0 = 不限">
          <Input inputMode="numeric" value={tpm} onChange={(e) => setTpm(e.target.value)} />
        </Field>
        <div className="flex items-end pb-1">
          <Checkbox label="设为默认分组" hint="未分组用户与新注册用户使用" checked={isDefault} onChange={(e) => setIsDefault(e.target.checked)} />
        </div>
        <Field label="可用模型" hint="不勾选 = 允许全部模型" className="col-span-2">
          <CheckboxGroup
            aria-label="可用模型"
            filterable
            options={modelIds.map((id) => ({
              value: id,
              label: <span className="font-mono text-xs">{id}</span>,
              text: `${id} ${displayName.get(id) ?? ''}`,
              hint: displayName.get(id) ?? (models.length > 0 ? '已删除' : undefined),
            }))}
            value={allowed}
            onChange={setAllowed}
            emptyText="暂无模型，请先在「模型与定价」中添加"
          />
        </Field>
        <div className="col-span-2">
          <FormError>{error}</FormError>
        </div>
      </form>
    </DialogContent>
  );
}
