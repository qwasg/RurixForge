import { useId, useState, type FormEvent } from 'react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Checkbox, Field, Input } from '@/components/ui/form';
import { KeyValueEditor, recordToRows, rowsToRecord, type KvRow } from '@/components/ui/kv-editor';
import { FormError } from '@/components/ui/misc';
import { isHttpUrl, parsePool, type PoolForm } from '@/lib/accounts';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { Account, Group, PatchAccountInput } from '@/lib/api/types';
import { useToast } from '@/lib/toast';
import { PlatformBadges, PoolFields } from './shared';

interface Props {
  account: Account | null;
  onOpenChange: (open: boolean) => void;
  groups: Group[];
  onSaved: (account: unknown) => void;
}

export function EditAccountDialog({ account, onOpenChange, groups, onSaved }: Props) {
  return (
    <Dialog open={account !== null} onOpenChange={onOpenChange}>
      {account ? <EditAccountForm account={account} groups={groups} onSaved={onSaved} onClose={() => onOpenChange(false)} /> : null}
    </Dialog>
  );
}

function EditAccountForm({
  account,
  groups,
  onSaved,
  onClose,
}: {
  account: Account;
  groups: Group[];
  onSaved: (account: unknown) => void;
  onClose: () => void;
}) {
  const formId = useId();
  const toast = useToast();
  const isApiKey = account.authType === 'apikey';
  const [name, setName] = useState(account.name);
  const [pool, setPool] = useState<PoolForm>({
    groupIds: account.groupIds ?? [],
    priority: String(account.priority),
    weight: String(account.weight),
    concurrencyLimit: String(account.concurrencyLimit),
    proxyUrl: account.proxyUrl ?? '',
  });
  const [mapping, setMapping] = useState<KvRow[]>(() => recordToRows(account.modelMapping));
  const [allowedModels, setAllowedModels] = useState((account.allowedModels ?? []).join(', '));
  const [baseUrl, setBaseUrl] = useState(account.baseUrl ?? '');
  const [apiKey, setApiKey] = useState('');
  const [supportsResponses, setSupportsResponses] = useState(account.supportsResponses);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return setError('请填写账号名称');
    const parsed = parsePool(pool);
    if (!parsed.ok) return setError(parsed.error);
    if (isApiKey && !isHttpUrl(baseUrl)) return setError('Base URL 需为 http(s) 地址');
    const input: PatchAccountInput = {
      name: name.trim(),
      priority: parsed.value.priority,
      weight: parsed.value.weight,
      concurrencyLimit: parsed.value.concurrencyLimit,
      groupIds: parsed.value.groupIds,
      modelMapping: rowsToRecord(mapping),
      allowedModels: [...new Set(allowedModels.split(/[\s,，]+/).filter(Boolean))],
      proxyUrl: parsed.value.proxyUrl,
    };
    if (isApiKey) {
      input.baseUrl = baseUrl.trim().replace(/\/+$/, '');
      input.supportsResponses = account.platform === 'openai' && supportsResponses;
      if (apiKey.trim()) input.apiKey = apiKey.trim();
    }
    setError(null);
    setBusy(true);
    try {
      const res = await adminApi.accounts.update(account.id, input);
      toast.success('账号已保存');
      onSaved(res);
      onClose();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogContent
      title={`编辑账号「${account.name}」`}
      description={[account.email, account.planType].filter(Boolean).join(' · ') || `ID ${account.id}`}
      size="lg"
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
      <form id={formId} onSubmit={onSubmit} className="flex flex-col gap-4" noValidate>
        <div className="flex items-end gap-3">
          <Field label="账号名称" required className="flex-1">
            <Input value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
          <div className="pb-1.5">
            <PlatformBadges account={account} />
          </div>
        </div>
        {isApiKey ? (
          <div className="grid grid-cols-2 gap-3">
            <Field label="Base URL" required className="col-span-2">
              <Input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} className="font-mono text-xs" spellCheck={false} />
            </Field>
            <Field label="替换 API Key" hint={account.keyHint ? `当前 Key 末 4 位：${account.keyHint}；留空则不修改` : '留空则不修改'}>
              <Input
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                autoComplete="off"
                className="font-mono text-xs"
                placeholder="留空则不修改"
              />
            </Field>
            {account.platform === 'openai' ? (
              <div className="flex items-end pb-1">
                <Checkbox
                  label="支持 /v1/responses"
                  checked={supportsResponses}
                  onChange={(e) => setSupportsResponses(e.target.checked)}
                />
              </div>
            ) : null}
          </div>
        ) : null}
        <PoolFields value={pool} onChange={setPool} groups={groups} showWeight />
        <Field label="可用模型范围" hint="填写模型 ID，以逗号或空格分隔；留空表示不限制。请求只会发给支持该模型的渠道。">
          <Input value={allowedModels} onChange={(e) => setAllowedModels(e.target.value)} className="font-mono text-xs" spellCheck={false} />
        </Field>
        <Field label="模型映射" hint="对外模型 ID → 该账号实际请求的上游模型名">
          <KeyValueEditor rows={mapping} onChange={setMapping} />
        </Field>
        <FormError>{error}</FormError>
      </form>
    </DialogContent>
  );
}
