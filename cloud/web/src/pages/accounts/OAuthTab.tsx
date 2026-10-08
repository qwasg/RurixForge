import { CircleCheck, ExternalLink, Link2, RefreshCw } from 'lucide-react';
import { useState, type FormEvent } from 'react';
import { Button } from '@/components/ui/button';
import { Field, Input, Select } from '@/components/ui/form';
import { CopyButton, FormError, Notice } from '@/components/ui/misc';
import { parseOAuthCallback, parsePool, type PoolForm } from '@/lib/accounts';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { Account, Group, OAuthExchangeInput, OAuthStartResult } from '@/lib/api/types';
import { formatDateTime } from '@/lib/format';
import { useNow } from '@/lib/hooks';
import { defaultPool, PoolFields } from './shared';

export const OAUTH_CALLBACK_PREFIX = 'http://localhost:1455/auth/callback';

export function OAuthTab({
  groups,
  accounts,
  onAdded,
  onDone,
}: {
  groups: Group[];
  /** 可重新授权的已有账号（取当前列表里的 OAuth 账号）。 */
  accounts: Account[];
  onAdded: () => void;
  onDone: () => void;
}) {
  const [session, setSession] = useState<OAuthStartResult | null>(null);
  const [starting, setStarting] = useState(false);
  const [callback, setCallback] = useState('');
  const [name, setName] = useState('');
  const [reauthId, setReauthId] = useState('');
  const [pool, setPool] = useState<PoolForm>(() => defaultPool(3));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [added, setAdded] = useState<Account | null>(null);
  const now = useNow(15_000);
  const oauthAccounts = accounts.filter((a) => a.authType === 'oauth');
  const expiresAt = session ? Date.parse(session.expiresAt) : Number.NaN;
  const expired = !Number.isNaN(expiresAt) && expiresAt <= now;

  const start = async () => {
    setStarting(true);
    setError(null);
    try {
      setSession(await adminApi.accounts.oauthStart());
      setCallback('');
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setStarting(false);
    }
  };

  const exchange = async (e: FormEvent) => {
    e.preventDefault();
    if (!session) return;
    const cb = parseOAuthCallback(callback);
    if ('error' in cb) return setError(cb.error);
    const parsed = parsePool(pool);
    if (!parsed.ok) return setError(parsed.error);
    const body: OAuthExchangeInput = {
      sessionId: session.sessionId,
      ...cb,
      groupIds: parsed.value.groupIds,
      priority: parsed.value.priority,
      concurrencyLimit: parsed.value.concurrencyLimit,
      proxyUrl: parsed.value.proxyUrl,
    };
    if (name.trim()) body.name = name.trim();
    if (reauthId) body.accountId = Number(reauthId);
    setError(null);
    setBusy(true);
    try {
      setAdded(await adminApi.accounts.oauthExchange(body));
      onAdded();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const reset = () => {
    setAdded(null);
    setSession(null);
    setCallback('');
    setName('');
    setReauthId('');
    setError(null);
  };

  if (added) {
    return (
      <div className="flex flex-col gap-3">
        <div className="flex items-center gap-2 text-sm">
          <CircleCheck className="size-4 text-success" />
          <span className="font-medium">{reauthId ? '重新授权成功' : '授权成功，已添加账号'}</span>
        </div>
        <div className="rounded-md border p-3 text-sm">
          <div className="font-medium">{added.name}</div>
          <div className="text-xs text-muted-foreground">
            {[added.email, added.planType, `ID ${added.id}`].filter(Boolean).join(' · ')}
          </div>
        </div>
        <div className="flex justify-end gap-2">
          <Button onClick={reset}>再授权一个</Button>
          <Button variant="primary" onClick={onDone}>
            完成
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <span className="flex size-5 items-center justify-center rounded-full bg-muted text-xs font-medium">1</span>
          <span className="text-sm font-medium">生成授权链接并登录</span>
        </div>
        {!session ? (
          <div>
            <Button variant="primary" onClick={start} loading={starting}>
              {starting ? null : <Link2 />}
              生成授权链接
            </Button>
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-1.5">
              <Input readOnly value={session.authUrl} aria-label="授权链接" className="font-mono text-xs" onFocus={(e) => e.currentTarget.select()} />
              <CopyButton text={session.authUrl} />
              <Button size="sm" onClick={() => window.open(session.authUrl, '_blank', 'noopener,noreferrer')}>
                <ExternalLink />
                打开
              </Button>
              <Button size="icon-sm" variant="ghost" aria-label="重新生成授权链接" title="重新生成" onClick={start} disabled={starting}>
                <RefreshCw />
              </Button>
            </div>
            <p className={expired ? 'text-xs text-danger' : 'text-xs text-muted-foreground'}>
              {expired ? '授权会话已过期，请重新生成链接。' : `链接有效期至 ${formatDateTime(session.expiresAt)}。`}
            </p>
          </div>
        )}
        <Notice>
          <ol className="list-inside list-decimal space-y-0.5">
            <li>点「打开」，在新标签页登录要接入的 ChatGPT 账号并同意授权。</li>
            <li>
              授权后浏览器会跳转到 <code className="font-mono">{OAUTH_CALLBACK_PREFIX}?code=…</code>
              ，该页面<strong>无法打开是正常的</strong>。
            </li>
            <li>从浏览器地址栏复制完整 URL，粘贴到下方，点「完成授权」。</li>
          </ol>
        </Notice>
      </div>

      <form onSubmit={exchange} className="flex flex-col gap-3" noValidate>
        <div className="flex items-center gap-2">
          <span className="flex size-5 items-center justify-center rounded-full bg-muted text-xs font-medium">2</span>
          <span className="text-sm font-medium">粘贴回调 URL</span>
        </div>
        <Field label="回调 URL" required>
          <Input
            value={callback}
            onChange={(e) => setCallback(e.target.value)}
            placeholder={`${OAUTH_CALLBACK_PREFIX}?code=…&state=…`}
            className="font-mono text-xs"
            disabled={!session}
            spellCheck={false}
          />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="账号名称（可选）" hint="留空则按邮箱命名">
            <Input value={name} onChange={(e) => setName(e.target.value)} disabled={!session} />
          </Field>
          <Field label="重新授权已有账号（可选）" hint="token 失效时对原账号重新授权，保留其配置">
            <Select value={reauthId} onChange={(e) => setReauthId(e.target.value)} disabled={!session}>
              <option value="">新建账号</option>
              {oauthAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                  {a.email ? `（${a.email}）` : ''}
                </option>
              ))}
            </Select>
          </Field>
        </div>
        <PoolFields value={pool} onChange={setPool} groups={groups} />
        <FormError>{error}</FormError>
        <div className="flex justify-end">
          <Button type="submit" variant="primary" loading={busy} disabled={!session || expired}>
            完成授权
          </Button>
        </div>
      </form>
    </div>
  );
}
