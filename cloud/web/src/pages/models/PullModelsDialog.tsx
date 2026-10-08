import { CloudDownload } from 'lucide-react';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Checkbox, CheckboxGroup, Field, Select } from '@/components/ui/form';
import { FormError, Notice } from '@/components/ui/misc';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { Account } from '@/lib/api/types';
import { useAsync } from '@/lib/hooks';
import { DEFAULT_POOL, defaultModelInput, POOL_LABEL } from '@/lib/models';
import { useToast } from '@/lib/toast';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  existingIds: Set<string>;
  onCreated: () => void;
}

export function PullModelsDialog({ open, onOpenChange, ...rest }: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open ? <PullModelsBody {...rest} onClose={() => onOpenChange(false)} /> : null}
    </Dialog>
  );
}

function PullModelsBody({ existingIds, onCreated, onClose }: Omit<Props, 'open' | 'onOpenChange'> & { onClose: () => void }) {
  const toast = useToast();
  const accounts = useAsync(() => adminApi.accounts.list({ limit: 200 }).then((r) => (r.items ?? []).filter((a) => a.authType === 'apikey')), []);
  const [accountId, setAccountId] = useState('');
  const [fetched, setFetched] = useState<{ account: Account; ids: string[] } | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [enable, setEnable] = useState(false);
  const [fetching, setFetching] = useState(false);
  const [creating, setCreating] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const pull = async () => {
    const account = (accounts.data ?? []).find((a) => String(a.id) === accountId);
    if (!account) return setError('请选择一个 API Key 账号');
    setError(null);
    setFetching(true);
    setFetched(null);
    setSelected([]);
    try {
      const res = await adminApi.accounts.models(account.id);
      const ids = [...new Set(res.items ?? [])].sort();
      setFetched({ account, ids });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setFetching(false);
    }
  };

  const create = async () => {
    if (!fetched || selected.length === 0) return;
    const { account } = fetched;
    const responses = account.platform === 'openai' && account.supportsResponses;
    const failures: string[] = [];
    setError(null);
    setCreating({ done: 0, total: selected.length });
    for (const [i, id] of selected.entries()) {
      try {
        await adminApi.models.create(defaultModelInput(id, account.platform, { responses, enabled: enable }));
      } catch (err) {
        failures.push(`${id}：${errorMessage(err)}`);
      }
      setCreating({ done: i + 1, total: selected.length });
    }
    setCreating(null);
    const ok = selected.length - failures.length;
    if (ok > 0) {
      toast.success(`已创建 ${ok} 个模型`);
      onCreated();
    }
    if (failures.length > 0) {
      setError(`${failures.length} 个创建失败：${failures.join('；')}`);
      setSelected((prev) => prev.filter((id) => failures.some((f) => f.startsWith(`${id}：`))));
    } else {
      onClose();
    }
  };

  const candidates = fetched?.ids.filter((id) => !existingIds.has(id)) ?? [];

  return (
    <DialogContent
      title="从账号拉取模型"
      description="读取 API Key 账号上游的 /models 列表，勾选后批量加入模型目录"
      size="lg"
      footer={
        <>
          <Button onClick={onClose} disabled={creating !== null}>
            取消
          </Button>
          <Button variant="primary" onClick={create} disabled={selected.length === 0} loading={creating !== null}>
            {creating ? `创建中 ${creating.done}/${creating.total}` : `批量创建（${selected.length}）`}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <div className="flex items-end gap-2">
          <Field label="API Key 账号" className="flex-1" hint="OAuth（Codex 订阅）账号不支持列出模型">
            <Select value={accountId} onChange={(e) => setAccountId(e.target.value)} disabled={accounts.loading}>
              <option value="">{accounts.loading ? '加载中…' : '选择账号…'}</option>
              {(accounts.data ?? []).map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name} · {a.baseUrl}
                </option>
              ))}
            </Select>
          </Field>
          <Button onClick={pull} loading={fetching} disabled={!accountId} className="mb-5">
            {fetching ? null : <CloudDownload />}
            拉取
          </Button>
        </div>
        {fetched ? (
          <>
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>
                上游共 {fetched.ids.length} 个模型，其中 {fetched.ids.length - candidates.length} 个已在目录中
              </span>
              <div className="flex gap-3">
                <button type="button" className="hover:text-foreground" onClick={() => setSelected(candidates)}>
                  全选未添加
                </button>
                <button type="button" className="hover:text-foreground" onClick={() => setSelected([])}>
                  清空
                </button>
              </div>
            </div>
            <CheckboxGroup
              aria-label="上游模型"
              filterable
              options={fetched.ids.map((id) => ({
                value: id,
                label: <span className="font-mono text-xs">{id}</span>,
                text: id,
                disabled: existingIds.has(id),
                hint: existingIds.has(id) ? '已存在' : undefined,
              }))}
              value={selected}
              onChange={setSelected}
              emptyText="上游没有返回模型"
              className="[&>div:nth-child(2)]:max-h-72"
            />
            <Checkbox
              label="创建后立即启用"
              hint="默认不启用：批量创建的模型单价为 0，定价后再启用，避免被免费调用"
              checked={enable}
              onChange={(e) => setEnable(e.target.checked)}
            />
            <Notice>
              默认值：显示名 = 模型 ID，上下文 128K，最大输出 16K，支持工具调用
              {fetched.account.platform === 'openai' && fetched.account.supportsResponses ? '，支持 Responses' : ''}，四项单价 0，用量池为
              {POOL_LABEL[DEFAULT_POOL]}。
            </Notice>
          </>
        ) : null}
        <FormError>{error}</FormError>
      </div>
    </DialogContent>
  );
}
