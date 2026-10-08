import { useCallback, useEffect, useState, type ReactNode } from 'react';
import {
  errorMessage,
  formatMoney,
  getCloudModels,
  type CloudCatalog,
} from '@/lib/accountApi';
import { useAccountStore } from '@/lib/accountStore';
import { useSettingsStore } from '@/lib/settingsStore';
import AdvancedSection from './AdvancedSection';
import { SetCard, SetRow, SetSectionLabel, SmBtn } from './controls';

export default function CloudModelsSection() {
  const status = useAccountStore((st) => st.status);
  const storeError = useAccountStore((st) => st.error);
  const refreshStatus = useAccountStore((st) => st.refreshStatus);
  const openAuth = useAccountStore((st) => st.openAuth);
  const setPage = useSettingsStore((st) => st.setPage);
  const [catalog, setCatalog] = useState<CloudCatalog | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const loggedIn = status?.loggedIn === true;

  useEffect(() => {
    if (useAccountStore.getState().status === null) void refreshStatus();
  }, [refreshStatus]);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setCatalog(await getCloudModels());
      setError(null);
    } catch (err) {
      setCatalog(null);
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (loggedIn) void load();
    else {
      setCatalog(null);
      setError(null);
    }
  }, [loggedIn, load]);

  const user = loggedIn ? status?.user : null;
  const plans = (status?.subscriptions ?? []).filter((s) => s.status === 'active').map((s) => s.planName);
  const models = Array.isArray(catalog?.models) ? catalog.models : [];

  return (
    <>
      <SetSectionLabel>RurixForge 云</SetSectionLabel>
      <div className="flex flex-col gap-3">
        <SetCard testId="cloud-account-card">
          {loggedIn ? (
            <SetRow
              title={<span data-testid="cloud-login-state">已登录 · {user?.nickname || user?.email}</span>}
              desc={`余额 ${formatMoney(status?.balanceMicros ?? 0, status?.currency)} · ${plans.length > 0 ? plans.join('、') : '无生效套餐'}${status?.reachable === false ? ' · 云端暂不可达' : ''}`}
              last
              control={<SmBtn label="账户" testId="cloud-open-account" onClick={() => setPage('account')} />}
            />
          ) : status ? (
            <SetRow
              title={<span data-testid="cloud-login-state">未登录</span>}
              desc="登录后即可使用云端模型,按用量计费,余额与套餐在「账户」页管理"
              last
              control={<SmBtn accent label="登录" testId="cloud-login" onClick={openAuth} />}
            />
          ) : (
            <SetRow
              title={<span data-testid="cloud-login-state">{storeError ? '无法读取云账户状态' : '正在读取云账户状态…'}</span>}
              desc={storeError ?? undefined}
              last
              control={storeError ? <SmBtn label="重试" onClick={() => void refreshStatus()} /> : undefined}
            />
          )}
        </SetCard>
        {loggedIn && (
          <SetCard testId="cloud-models-card">
            {error ? (
              <div data-testid="cloud-models-error" className="flex items-center gap-2 px-4 py-3 text-[11.5px] text-warn">
                <span className="min-w-0 flex-1">云端模型目录加载失败:{error}</span>
                <SmBtn label={loading ? '重试中…' : '重试'} disabled={loading} onClick={() => void load()} />
              </div>
            ) : catalog === null ? (
              <div className="px-4 py-3 text-[11.5px] text-fg-4">加载中…</div>
            ) : models.length === 0 ? (
              <div data-testid="cloud-models-empty" className="px-4 py-3 text-[11.5px] text-fg-4">
                当前分组暂无可用模型
              </div>
            ) : (
              <div data-testid="cloud-models-names" className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,180px),1fr))] gap-x-4 gap-y-2 px-4 py-3">
                {models.map((model) => <span key={model.id} data-testid={`cloud-model-${model.id}`} className={`break-words text-[12px] leading-6 ${model.available === false ? 'text-fg-4' : 'text-fg-2'}`} title={`${model.displayName || model.id}${model.id === catalog.defaultModel ? ' · 默认模型' : ''}${model.available === false ? ' · 暂不可用' : ''}`}>{model.displayName || model.id}</span>)}
              </div>
            )}
          </SetCard>
        )}
      </div>
    </>
  );
}

/** 自带密钥渠道的折叠区:默认收起;云端禁用自带密钥(byoAllowed=false)时整块不渲染。 */
export function ByoAdvancedSection({ children }: { children: ReactNode }) {
  const byoAllowed = useAccountStore((st) => st.status?.byoAllowed !== false);
  if (!byoAllowed) return null;
  return (
    <AdvancedSection title="高级：自带密钥" testId="byo-advanced">
      {children}
    </AdvancedSection>
  );
}
