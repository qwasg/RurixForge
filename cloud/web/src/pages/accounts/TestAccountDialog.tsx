import { CircleCheck, CircleX, Play } from 'lucide-react';
import { useId, useState, type FormEvent } from 'react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Field, Input } from '@/components/ui/form';
import { FormError } from '@/components/ui/misc';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { Account, AccountTestResult, AdminModel } from '@/lib/api/types';
import { formatNumber } from '@/lib/format';
import { useSettings } from '@/lib/settings';

interface Props {
  account: Account | null;
  onOpenChange: (open: boolean) => void;
  models: AdminModel[];
}

export function TestAccountDialog({ account, onOpenChange, models }: Props) {
  return (
    <Dialog open={account !== null} onOpenChange={onOpenChange}>
      {account ? <TestForm account={account} models={models} /> : null}
    </Dialog>
  );
}

function TestForm({ account, models }: { account: Account; models: AdminModel[] }) {
  const listId = useId();
  const { settings } = useSettings();
  const suggestions = [
    ...new Set([
      ...(account.allowedModels ?? []),
      ...models.filter((m) => m.platform === account.platform && m.enabled &&
        (!account.allowedModels?.length || account.allowedModels.includes(m.id) ||
          account.allowedModels.includes(account.modelMapping?.[m.id] || m.upstreamModel || m.id))).map((m) => m.id),
    ]),
  ];
  const fallback = account.platform === 'openai' ? settings?.defaultModel ?? '' : '';
  const [model, setModel] = useState(suggestions[0] ?? fallback);
  const [result, setResult] = useState<AccountTestResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!model.trim()) return setError('请填写模型');
    setError(null);
    setResult(null);
    setBusy(true);
    try {
      setResult(await adminApi.accounts.test(account.id, model.trim()));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogContent title={`测试账号「${account.name}」`} description="向上游发送一个最小请求，检查凭据与连通性" size="sm">
      <form onSubmit={onSubmit} className="flex flex-col gap-3" noValidate>
        <Field label="模型" hint="可选列表来自该账号的模型映射与模型目录">
          <Input list={listId} value={model} onChange={(e) => setModel(e.target.value)} className="font-mono text-xs" autoFocus />
        </Field>
        <datalist id={listId}>
          {suggestions.map((s) => (
            <option key={s} value={s} />
          ))}
        </datalist>
        <div className="flex justify-end">
          <Button type="submit" variant="primary" loading={busy}>
            {busy ? null : <Play />}
            开始测试
          </Button>
        </div>
        <FormError>{error}</FormError>
        {result ? (
          <div
            className={
              result.ok
                ? 'rounded-md border border-success/30 bg-success/5 p-3 text-sm'
                : 'rounded-md border border-danger/30 bg-danger/5 p-3 text-sm'
            }
          >
            <div className="flex items-center gap-2 font-medium">
              {result.ok ? <CircleCheck className="size-4 text-success" /> : <CircleX className="size-4 text-danger" />}
              {result.ok ? '连通正常' : '测试失败'}
              <span className="ml-auto text-xs font-normal text-muted-foreground tabular">
                HTTP {result.httpStatus || '—'} · {formatNumber(result.latencyMs)} ms
              </span>
            </div>
            {result.message ? <p className="mt-1 break-words text-xs text-muted-foreground">{result.message}</p> : null}
          </div>
        ) : null}
      </form>
    </DialogContent>
  );
}
