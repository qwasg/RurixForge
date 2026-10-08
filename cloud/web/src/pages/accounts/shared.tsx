import { Snowflake } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { CheckboxGroup, Field, Input } from '@/components/ui/form';
import { QuotaBar } from '@/components/ui/misc';
import { ACCOUNT_STATUS_LABEL, cooldownRemaining, PLATFORM_LABEL, quotaBars, type PoolForm } from '@/lib/accounts';
import type { Account, AccountQuota, Group } from '@/lib/api/types';
import { formatDateTime, formatDuration } from '@/lib/format';
import { useNow } from '@/lib/hooks';
import { cn } from '@/lib/utils';

export function defaultPool(concurrencyLimit: number): PoolForm {
  return { groupIds: [], priority: '0', weight: '1', concurrencyLimit: String(concurrencyLimit), proxyUrl: '' };
}

/** 账号池参数：分组、优先级、（权重）、并发、代理。导入类接口没有权重字段。 */
export function PoolFields({
  value,
  onChange,
  groups,
  showWeight = false,
}: {
  value: PoolForm;
  onChange: (next: PoolForm) => void;
  groups: Group[];
  showWeight?: boolean;
}) {
  const set = (patch: Partial<PoolForm>) => onChange({ ...value, ...patch });
  return (
    <div className={cn('grid gap-3', showWeight ? 'grid-cols-3' : 'grid-cols-2')}>
      <Field label="优先级" hint="数值越小越优先">
        <Input inputMode="numeric" value={value.priority} onChange={(e) => set({ priority: e.target.value })} />
      </Field>
      {showWeight ? (
        <Field label="权重" hint="同优先级内按权重随机">
          <Input inputMode="numeric" value={value.weight} onChange={(e) => set({ weight: e.target.value })} />
        </Field>
      ) : null}
      <Field label="并发上限" hint="该账号同时处理的请求数">
        <Input inputMode="numeric" value={value.concurrencyLimit} onChange={(e) => set({ concurrencyLimit: e.target.value })} />
      </Field>
      <Field label="分组" className={showWeight ? 'col-span-3' : 'col-span-2'} hint="账号参与所选分组的调度">
        <CheckboxGroup
          aria-label="分组"
          inline
          options={groups.map((g) => ({ value: g.id, label: g.name, hint: g.isDefault ? '默认' : undefined }))}
          value={value.groupIds}
          onChange={(groupIds) => set({ groupIds })}
          emptyText="暂无分组"
        />
      </Field>
      <Field
        label="代理 URL（可选）"
        className={showWeight ? 'col-span-3' : 'col-span-2'}
        hint="留空则使用全局上游代理（FORGE_CLOUD_UPSTREAM_PROXY）"
      >
        <Input
          value={value.proxyUrl}
          onChange={(e) => set({ proxyUrl: e.target.value })}
          placeholder="http://127.0.0.1:7890 或 socks5://127.0.0.1:1080"
          spellCheck={false}
        />
      </Field>
    </div>
  );
}

export function PlatformBadges({ account }: { account: Pick<Account, 'platform' | 'authType'> }) {
  return (
    <div className="flex flex-wrap items-center gap-1">
      <Badge tone="outline">{PLATFORM_LABEL[account.platform] ?? account.platform}</Badge>
      {account.authType === 'oauth' ? <Badge tone="info">OAuth</Badge> : <Badge tone="neutral">API Key</Badge>}
    </div>
  );
}

export function AccountStatusBadge({ account }: { account: Account }) {
  const tone = account.status === 'active' ? 'success' : account.status === 'error' ? 'danger' : 'neutral';
  return <Badge tone={tone}>{ACCOUNT_STATUS_LABEL[account.status] ?? account.status}</Badge>;
}

/** 冷却倒计时（每秒刷新，只在冷却中的行上计时）。 */
export function CooldownCell({ account }: { account: Account }) {
  const initial = cooldownRemaining(account, Date.now());
  if (initial <= 0) return <span className="text-muted-foreground">—</span>;
  return <CooldownTicker account={account} />;
}

function CooldownTicker({ account }: { account: Account }) {
  const now = useNow(1000);
  const left = cooldownRemaining(account, now);
  if (left <= 0) return <span className="text-muted-foreground">—</span>;
  return (
    <Badge tone="warning" title={`冷却至 ${formatDateTime(account.cooldownUntil)}`}>
      <Snowflake />
      {formatDuration(left)}
    </Badge>
  );
}

export function QuotaBars({ quota }: { quota: AccountQuota | null }) {
  const bars = quotaBars(quota);
  if (bars.length === 0) return <span className="text-muted-foreground">—</span>;
  const now = Date.now();
  return (
    <div className="flex flex-col gap-1">
      {bars.map((b) => {
        let title = `${b.label}窗口已用 ${b.percent}%`;
        if (b.resetAt !== null) {
          const at = formatDateTime(new Date(b.resetAt).toISOString());
          title += b.resetAt > now ? ` · ${formatDuration((b.resetAt - now) / 1000)}后重置（${at}）` : ` · 重置时间 ${at}`;
        }
        return <QuotaBar key={b.key} label={b.label} percent={b.percent} title={title} />;
      })}
    </div>
  );
}

export function GroupNames({ ids, names }: { ids: number[]; names: Map<number, string> }) {
  if (!ids || ids.length === 0) return <span className="text-muted-foreground">—</span>;
  const labels = ids.map((id) => names.get(id) ?? `#${id}`);
  return (
    <span className="text-xs" title={labels.join('\n')}>
      {labels.slice(0, 2).join('、')}
      {labels.length > 2 ? ` +${labels.length - 2}` : ''}
    </span>
  );
}
