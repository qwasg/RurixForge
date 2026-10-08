import { useEffect } from 'react';
import { useAccountStore } from '@/lib/accountStore';
import { formatMoney } from '@/lib/accountApi';
import { getMembership } from '@/lib/billingApi';
import { Loading, SectionError, useLoader } from './billing/parts';
import UsageDashboard from './UsageDashboard';
import { SetCard, SetH1, SetRow, SmBtn } from './controls';

function CycleUsage({ userId }: { userId: number }) {
  const membership = useLoader(getMembership, true);
  const m = membership.data;
  const validCycle = m && Number.isFinite(Date.parse(m.cycle.start)) &&
    Number.isFinite(Date.parse(m.cycle.end)) && Date.parse(m.cycle.end) > Date.parse(m.cycle.start);
  return (
    <>
      {membership.error && <SetCard><SectionError message={`读取用量周期失败：${membership.error}`}
        onRetry={() => void membership.reload()} testId="billing-membership-error" /></SetCard>}
      {m ? (
        <>
          <div data-testid="billing-overview" className="mb-5 grid gap-3 sm:grid-cols-3">
            {[
              ['当前套餐', m.tier.name],
              ['第三方模型额度', formatMoney(m.pools.api.remainingMicros, m.currency)],
              ['平台模型额度', formatMoney(m.pools.forge.remainingMicros, m.currency)],
            ].map(([label, value]) => (
              <SetCard key={label}>
                <div className="px-5 py-4">
                  <div className="mb-2 text-[11px] text-fg-3">{label}</div>
                  <div className="font-code text-[19px] text-fg">{value}</div>
                </div>
              </SetCard>
            ))}
          </div>
          {validCycle ? (
            <UsageDashboard key={`${userId}:${m.cycle.start}:${m.cycle.end}`}
              currency={m.currency} from={m.cycle.start} to={m.cycle.end} testId="billing-usage" />
          ) : (
            <SetCard><SectionError message="后台尚未返回有效的用量周期" onRetry={() => void membership.reload()} testId="billing-usage-error" /></SetCard>
          )}
        </>
      ) : !membership.error && <SetCard><Loading /></SetCard>}
    </>
  );
}

/** Read-only membership context and every cloud model call in the actual billing cycle. */
export default function BillingPage() {
  const status = useAccountStore((state) => state.status);
  const loading = useAccountStore((state) => state.loading);
  const storeError = useAccountStore((state) => state.error);
  const refreshStatus = useAccountStore((state) => state.refreshStatus);
  const openAuth = useAccountStore((state) => state.openAuth);
  const loggedIn = status?.loggedIn === true && status.user !== null;

  useEffect(() => { void refreshStatus(); }, [refreshStatus]);

  return (
    <div data-testid="settings-page-billing" className="flex flex-col">
      <SetH1>套餐与用量</SetH1>
      {!loggedIn ? (
        !status && !loading && storeError ? (
          <SetCard testId="billing-status-error">
            <SetRow title="无法读取账户状态" desc={storeError} last
              control={<SmBtn label="重试" testId="billing-status-retry" onClick={() => void refreshStatus()} />} />
          </SetCard>
        ) : (
          <SetCard testId="billing-logged-out">
            <SetRow title="RurixForge 云" desc="登录后查看当前额度、逐次模型调用与消耗。" last
              control={<SmBtn accent label="登录" testId="billing-login" onClick={openAuth} />} />
          </SetCard>
        )
      ) : (
        <>
          {status?.reachable === false && <div className="mb-3 text-[12px] text-warn">云端暂不可达，以下数据可能不是最新</div>}
          <CycleUsage key={status?.user?.id} userId={status!.user!.id} />
        </>
      )}
    </div>
  );
}
