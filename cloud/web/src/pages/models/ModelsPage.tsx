import { CloudDownload, Pencil, Plus, RefreshCw, Search, Trash2 } from 'lucide-react';
import { useMemo, useState } from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { useConfirm } from '@/components/ui/confirm';
import { Input, Select } from '@/components/ui/form';
import { Card, PageHeader, Toolbar } from '@/components/ui/misc';
import { Table, TableStatus, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { PLATFORM_LABEL } from '@/lib/accounts';
import { adminApi } from '@/lib/api/admin';
import type { AdminModel, Platform } from '@/lib/api/types';
import { formatAmount, formatCompact } from '@/lib/format';
import { POOL_LABEL, PRICE_FIELDS } from '@/lib/models';
import { useModels } from '@/lib/queries';
import { useSettings } from '@/lib/settings';
import { useToast } from '@/lib/toast';
import { ModelDialog } from './ModelDialog';
import { PullModelsDialog } from './PullModelsDialog';

function Capabilities({ m }: { m: AdminModel }) {
  const c = m.capabilities;
  return (
    <div className="flex max-w-56 flex-wrap gap-1">
      {c.vision ? <Badge tone="info">视觉</Badge> : null}
      {c.tools ? <Badge tone="neutral">工具</Badge> : null}
      {c.responses ? <Badge tone="success">Responses</Badge> : null}
      {(c.reasoningEfforts ?? []).length > 0 ? <Badge tone="outline">思考 {c.reasoningEfforts.join('/')}</Badge> : null}
    </div>
  );
}

export function ModelsPage() {
  const models = useModels();
  const toast = useToast();
  const { currency } = useSettings();
  const [confirmEl, confirm] = useConfirm();
  const [q, setQ] = useState('');
  const [platform, setPlatform] = useState<Platform | ''>('');
  const [editing, setEditing] = useState<AdminModel | 'new' | null>(null);
  const [pullOpen, setPullOpen] = useState(false);

  const all = models.data ?? [];
  const existingIds = useMemo(() => new Set(all.map((m) => m.id)), [all]);
  const items = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return all
      .filter((m) => !platform || m.platform === platform)
      .filter((m) => !needle || [m.id, m.displayName, m.upstreamModel].some((s) => s?.toLowerCase().includes(needle)))
      .sort((a, b) => a.sort - b.sort || a.id.localeCompare(b.id));
  }, [all, q, platform]);

  return (
    <div>
      <PageHeader
        title="模型与定价"
        description="对外模型目录：客户端请求的 model 必须在此列出；单价按每 1M tokens 计，实际扣费再乘分组倍率。"
        actions={
          <>
            <Button size="sm" onClick={() => setPullOpen(true)}>
              <CloudDownload />
              从账号拉取模型
            </Button>
            <Button size="sm" variant="primary" onClick={() => setEditing('new')}>
              <Plus />
              新增模型
            </Button>
          </>
        }
      />
      <Card>
        <Toolbar>
          <div className="relative w-60">
            <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="搜索 ID / 显示名 / 上游模型" aria-label="搜索模型" className="pl-7" />
          </div>
          <Select aria-label="平台筛选" className="w-32" value={platform} onChange={(e) => setPlatform(e.target.value as Platform | '')}>
            <option value="">全部平台</option>
            <option value="openai">OpenAI</option>
            <option value="anthropic">Anthropic</option>
          </Select>
          <span className="text-xs text-muted-foreground">
            {items.length} / {all.length} 个
          </span>
          <Button variant="ghost" size="sm" className="ml-auto" onClick={models.reload} loading={models.loading && !!models.data}>
            {models.loading && models.data ? null : <RefreshCw />}
            刷新
          </Button>
        </Toolbar>
        <Table>
          <THead>
            <tr>
              <TH>模型 ID</TH>
              <TH>平台</TH>
              <TH>用量池</TH>
              <TH>上游模型</TH>
              <TH>能力</TH>
              <TH className="text-right">上下文 / 输出</TH>
              <TH>单价（{currency} / 1M）</TH>
              <TH>状态</TH>
              <TH className="text-right">排序</TH>
              <TH className="text-right">可用账号</TH>
              <TH className="text-right">操作</TH>
            </tr>
          </THead>
          <TBody>
            <TableStatus
              colSpan={11}
              loading={models.loading}
              error={models.error}
              empty={items.length === 0}
              emptyText={all.length === 0 ? '模型目录为空，点「新增模型」或「从账号拉取模型」' : '没有匹配的模型'}
              onRetry={models.reload}
            />
            {items.map((m) => (
              <TR key={m.id}>
                <TD>
                  <div className="font-mono text-xs font-medium">{m.id}</div>
                  {m.displayName && m.displayName !== m.id ? <div className="text-[11px] text-muted-foreground">{m.displayName}</div> : null}
                </TD>
                <TD>
                  <Badge tone="outline">{PLATFORM_LABEL[m.platform] ?? m.platform}</Badge>
                </TD>
                <TD>
                  <Badge tone={m.pool === 'forge' ? 'success' : 'info'}>{POOL_LABEL[m.pool] ?? m.pool ?? '—'}</Badge>
                </TD>
                <TD className="font-mono text-xs text-muted-foreground">{m.upstreamModel || '同 ID'}</TD>
                <TD>
                  <Capabilities m={m} />
                </TD>
                <TD className="tabular whitespace-nowrap text-right text-xs">
                  {formatCompact(m.capabilities.contextWindow)} / {formatCompact(m.capabilities.maxOutput)}
                </TD>
                <TD>
                  <div className="tabular grid grid-cols-[auto_auto] gap-x-3 whitespace-nowrap text-xs">
                    {PRICE_FIELDS.map(({ key, short }) => (
                      <span key={key}>
                        <span className="text-muted-foreground">{short} </span>
                        {formatAmount(m.pricing[key])}
                      </span>
                    ))}
                  </div>
                </TD>
                <TD>
                  <div className="flex flex-col items-start gap-1">
                    {m.enabled ? <Badge tone="success">启用</Badge> : <Badge tone="neutral">停用</Badge>}
                    {m.isDefault ? <Badge tone="info">默认</Badge> : null}
                  </div>
                </TD>
                <TD className="tabular text-right">{m.sort}</TD>
                <TD className="text-right">
                  <Badge tone={m.availableAccounts > 0 ? 'success' : 'warning'} title={m.availableAccounts > 0 ? undefined : '没有可调度的健康账号'}>
                    {m.availableAccounts}
                  </Badge>
                </TD>
                <TD>
                  <div className="flex justify-end gap-1">
                    <Button size="icon-sm" variant="ghost" aria-label={`编辑 ${m.id}`} onClick={() => setEditing(m)}>
                      <Pencil />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="danger-ghost"
                      aria-label={`删除 ${m.id}`}
                      onClick={() =>
                        confirm({
                          title: `删除模型「${m.id}」`,
                          description: '删除后客户端将无法再请求该模型；历史用量记录不受影响。',
                          confirmText: '删除',
                          danger: true,
                          action: async () => {
                            await adminApi.models.remove(m.id);
                            toast.success('模型已删除');
                            models.reload();
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

      <ModelDialog model={editing} onOpenChange={(o) => !o && setEditing(null)} onSaved={models.reload} />
      <PullModelsDialog open={pullOpen} onOpenChange={setPullOpen} existingIds={existingIds} onCreated={models.reload} />
      {confirmEl}
    </div>
  );
}
