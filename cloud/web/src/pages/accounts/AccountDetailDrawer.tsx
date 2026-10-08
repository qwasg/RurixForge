import { Gauge } from 'lucide-react';
import type { ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import { Dialog, DrawerContent } from '@/components/ui/dialog';
import { DescList, JsonBlock } from '@/components/ui/misc';
import { Table, TBody, TD, TH, THead, TR } from '@/components/ui/table';
import { PLATFORM_LABEL } from '@/lib/accounts';
import type { Account } from '@/lib/api/types';
import { formatDateTime, formatRelative } from '@/lib/format';
import { AccountStatusBadge, CooldownCell, GroupNames, PlatformBadges, QuotaBars } from './shared';

interface Props {
  account: Account | null;
  onOpenChange: (open: boolean) => void;
  groupNames: Map<number, string>;
  onRefreshQuota: (account: Account) => void;
  refreshingQuota: boolean;
}

function Section({ title, children, actions }: { title: string; children: ReactNode; actions?: ReactNode }) {
  return (
    <section className="mt-5">
      <div className="mb-2 flex items-center justify-between">
        <h3 className="text-sm font-semibold">{title}</h3>
        {actions}
      </div>
      {children}
    </section>
  );
}

export function AccountDetailDrawer({ account, onOpenChange, groupNames, onRefreshQuota, refreshingQuota }: Props) {
  return (
    <Dialog open={account !== null} onOpenChange={onOpenChange}>
      {account ? (
        <DrawerContent
          title={account.name}
          description={`ID ${account.id} · ${PLATFORM_LABEL[account.platform] ?? account.platform} · ${
            account.authType === 'oauth' ? 'OAuth（Codex 订阅）' : 'API Key'
          }`}
        >
          <DescList
            items={[
              { label: '类型', value: <PlatformBadges account={account} /> },
              {
                label: '状态',
                value: (
                  <div className="flex items-center gap-2">
                    <AccountStatusBadge account={account} />
                    <CooldownCell account={account} />
                  </div>
                ),
              },
              { label: '邮箱 / 套餐', value: [account.email, account.planType].filter(Boolean).join(' · ') || '—' },
              { label: 'Base URL', value: account.baseUrl ? <span className="font-mono text-xs">{account.baseUrl}</span> : '—' },
              { label: 'API Key', value: account.keyHint ? <span className="font-mono text-xs">••••{account.keyHint}</span> : '—' },
              { label: '优先级 / 权重', value: `${account.priority} / ${account.weight}` },
              { label: '并发', value: `${account.currentConcurrency} / ${account.concurrencyLimit}` },
              { label: '分组', value: <GroupNames ids={account.groupIds} names={groupNames} /> },
              { label: 'Responses', value: account.supportsResponses ? '支持 /v1/responses' : '不支持' },
              { label: '代理', value: account.proxyUrl ? <span className="font-mono text-xs">{account.proxyUrl}</span> : '全局默认' },
              {
                label: '最近错误',
                value: account.lastError ? (
                  <div>
                    <div className="whitespace-pre-wrap break-words text-danger">{account.lastError}</div>
                    <div className="text-xs text-muted-foreground">{formatDateTime(account.lastErrorAt)}</div>
                  </div>
                ) : (
                  '—'
                ),
              },
              {
                label: 'Token 过期',
                value: account.tokenExpiresAt
                  ? `${formatDateTime(account.tokenExpiresAt)}（${formatRelative(account.tokenExpiresAt)}）`
                  : '—',
              },
              { label: '最近刷新', value: formatDateTime(account.lastRefreshAt) },
              { label: '最近使用', value: formatDateTime(account.lastUsedAt) },
              { label: '创建 / 更新', value: `${formatDateTime(account.createdAt)} / ${formatDateTime(account.updatedAt)}` },
            ]}
          />

          <Section title="可用模型范围">
            {account.allowedModels?.length ? (
              <div className="flex flex-col gap-1 font-mono text-xs">
                {account.allowedModels.map((id) => <span key={id}>{id}</span>)}
              </div>
            ) : (
              <p className="text-xs text-muted-foreground">不限制模型</p>
            )}
          </Section>

          <Section title="模型映射">
            {Object.keys(account.modelMapping ?? {}).length === 0 ? (
              <p className="text-xs text-muted-foreground">未配置（按模型目录的上游模型名请求）</p>
            ) : (
              <div className="rounded-md border">
                <Table>
                  <THead>
                    <tr>
                      <TH>对外模型 ID</TH>
                      <TH>上游模型名</TH>
                    </tr>
                  </THead>
                  <TBody>
                    {Object.entries(account.modelMapping).map(([k, v]) => (
                      <TR key={k}>
                        <TD className="font-mono text-xs">{k}</TD>
                        <TD className="font-mono text-xs">{v}</TD>
                      </TR>
                    ))}
                  </TBody>
                </Table>
              </div>
            )}
          </Section>

          <Section
            title="额度窗口"
            actions={
              account.authType === 'oauth' ? (
                <Button size="sm" onClick={() => onRefreshQuota(account)} loading={refreshingQuota}>
                  {refreshingQuota ? null : <Gauge />}
                  刷新额度
                </Button>
              ) : null
            }
          >
            <QuotaBars quota={account.quota} />
            {account.quota?.updatedAt ? (
              <p className="mt-1 text-xs text-muted-foreground">快照时间 {formatDateTime(String(account.quota.updatedAt))}</p>
            ) : null}
          </Section>

          <Section title="wham/usage 原始响应（quota.usage）">
            {account.quota?.usage === undefined || account.quota?.usage === null ? (
              <p className="text-xs text-muted-foreground">
                暂无。{account.authType === 'oauth' ? '点「刷新额度」向上游查询。' : '仅 Codex 订阅账号提供。'}
              </p>
            ) : (
              <JsonBlock value={account.quota.usage} />
            )}
          </Section>

          <Section title="完整 quota 快照">
            <details className="text-xs">
              <summary className="cursor-pointer text-muted-foreground hover:text-foreground">展开 JSON</summary>
              <JsonBlock value={account.quota ?? null} className="mt-2" />
            </details>
          </Section>
        </DrawerContent>
      ) : null}
    </Dialog>
  );
}
