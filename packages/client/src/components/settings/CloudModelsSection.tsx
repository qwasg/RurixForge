import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { ArrowUpRight, Brain, Check, Eye, Search, Wrench } from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  errorMessage,
  formatAmount,
  formatMoney,
  getCloudModels,
  type CloudCatalog,
  type CloudModel,
} from '@/lib/accountApi';
import { useAccountStore } from '@/lib/accountStore';
import { formatWindow } from '@/lib/customModels';
import { switchToLocalModel, useCurrentLocalModelId } from '@/lib/modelSwitch';
import { useSettingsStore } from '@/lib/settingsStore';
import { useToastStore } from '@/lib/toastStore';
import AdvancedSection from './AdvancedSection';
import { SetCard, SetRow, SetSectionLabel, SmBtn } from './controls';
import './cloudModels.css';

/** 模型 id 前缀 → 厂商(目录的 platform 是上游协议,Gemini/GLM 也会标 openai,不能拿来分组)。 */
const VENDORS: Array<{ name: string; test: RegExp }> = [
  { name: 'OpenAI', test: /^(gpt|chatgpt|codex|o\d)/ },
  { name: 'Anthropic', test: /^claude/ },
  { name: 'Google', test: /^(gemini|gemma)/ },
  { name: 'DeepSeek', test: /^deepseek/ },
  { name: '智谱 GLM', test: /^(glm|chatglm)/ },
  { name: 'Kimi', test: /^(kimi|moonshot)/ },
  { name: '通义千问', test: /^(qwen|qwq)/ },
  { name: '小米 MiMo', test: /^mimo/ },
  { name: 'xAI', test: /^grok/ },
  { name: 'MiniMax', test: /^(minimax|abab)/ },
  { name: '豆包', test: /^doubao/ },
  { name: 'Mistral', test: /^(mistral|codestral)/ },
  { name: 'Meta', test: /^llama/ },
];

export function modelVendor(m: Pick<CloudModel, 'id' | 'platform'>): string {
  const id = m.id.toLowerCase();
  const hit = VENDORS.find((v) => v.test.test(id));
  if (hit) return hit.name;
  const p = (m.platform ?? '').trim();
  return p ? p.charAt(0).toUpperCase() + p.slice(1) : '其他';
}

function priceText(m: CloudModel, currency: string): string | null {
  const p = m.pricing;
  if (!p || (!p.inputPer1M && !p.outputPer1M)) return null;
  return `输入 ${formatAmount(p.inputPer1M)} / 输出 ${formatAmount(p.outputPer1M)} ${currency || 'USD'} 每百万 tokens`;
}

function Cap({ icon, children, title }: { icon: ReactNode; children: ReactNode; title?: string }) {
  return (
    <span className="cml-cap" title={title}>
      {icon}
      {children}
    </span>
  );
}

function ModelTile({
  model,
  currency,
  isDefault,
  current,
}: {
  model: CloudModel;
  currency: string;
  isDefault: boolean;
  current: boolean;
}) {
  const [busy, setBusy] = useState(false);
  const caps = model.capabilities;
  const name = model.displayName || model.id;
  const unavailable = model.available === false;
  const efforts = Array.isArray(caps?.reasoningEfforts) ? caps.reasoningEfforts : [];
  const price = priceText(model, currency);
  const meta = [
    caps?.contextWindow ? `${formatWindow(caps.contextWindow)} 上下文` : null,
    caps?.maxOutput ? `${formatWindow(caps.maxOutput)} 输出` : null,
  ].filter(Boolean);
  const use = async () => {
    setBusy(true);
    try {
      await switchToLocalModel(`cloud:${model.id}`);
      useToastStore.getState().push('success', `已切换到 ${name}`);
    } catch (err) {
      useToastStore.getState().push('error', errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <article
      className={cn('cml-tile', current && 'is-current', unavailable && 'is-unavailable')}
      data-testid={`cloud-model-${model.id}`}
      title={`${name}(${model.id})${isDefault ? ' · 默认模型' : ''}${unavailable ? ' · 暂不可用' : ''}`}
    >
      <div className="cml-tile-top">
        <span className="cml-vendor">{modelVendor(model)}</span>
        <span className="cml-badges">
          {isDefault && <span className="cml-badge">默认</span>}
          {unavailable && <span className="cml-badge is-muted">暂不可用</span>}
        </span>
      </div>
      <h4 className="cml-name">{name}</h4>
      {meta.length > 0 && <p className="cml-meta">{meta.join(' · ')}</p>}
      {price && <p className="cml-price">{price}</p>}
      <div className="cml-tile-foot">
        <span className="cml-caps">
          {caps?.vision && <Cap icon={<Eye size={12} />}>视觉</Cap>}
          {efforts.length > 0 && (
            <Cap icon={<Brain size={12} />} title={`推理强度:${efforts.join(' / ')}`}>
              推理
            </Cap>
          )}
          {caps?.tools && <Cap icon={<Wrench size={12} />}>工具</Cap>}
        </span>
        <button
          type="button"
          className={cn('cml-use', current && 'is-current')}
          disabled={busy || unavailable || current}
          data-testid={`cloud-model-use-${model.id}`}
          onClick={() => void use()}
        >
          {current ? <Check size={12} /> : <ArrowUpRight size={12} />}
          {current ? '使用中' : '使用'}
        </button>
      </div>
    </article>
  );
}

function CloudModelList({ catalog }: { catalog: CloudCatalog }) {
  const models = useMemo(() => (Array.isArray(catalog.models) ? catalog.models : []), [catalog]);
  const [query, setQuery] = useState('');
  const [vendor, setVendor] = useState<string | null>(null);
  const currentId = useCurrentLocalModelId();
  const vendors = useMemo(() => {
    const out: Array<{ name: string; count: number }> = [];
    for (const m of models) {
      const name = modelVendor(m);
      const hit = out.find((v) => v.name === name);
      if (hit) hit.count += 1;
      else out.push({ name, count: 1 });
    }
    return out;
  }, [models]);
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return models.filter(
      (m) =>
        (vendor === null || modelVendor(m) === vendor) &&
        (q === '' ||
          m.id.toLowerCase().includes(q) ||
          (m.displayName ?? '').toLowerCase().includes(q) ||
          modelVendor(m).toLowerCase().includes(q)),
    );
  }, [models, query, vendor]);
  const available = models.filter((m) => m.available !== false).length;
  const defaultName = models.find((m) => m.id === catalog.defaultModel)?.displayName;

  return (
    <div className="cml" data-testid="cloud-models-list">
      <div className="cml-head">
        <div>
          <strong>云端模型</strong>
          <span data-testid="cloud-models-summary">
            {available}/{models.length} 可用{defaultName ? ` · 默认 ${defaultName}` : ''}
          </span>
        </div>
        <label className="cml-search">
          <Search size={13} aria-hidden />
          <input
            value={query}
            placeholder="搜索模型"
            aria-label="搜索云端模型"
            data-testid="cloud-models-search"
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
      </div>
      {vendors.length > 1 && (
        <div className="cml-filters" role="radiogroup" aria-label="按厂商筛选">
          <button
            type="button"
            role="radio"
            aria-checked={vendor === null}
            className={cn('cml-filter', vendor === null && 'is-selected')}
            onClick={() => setVendor(null)}
          >
            全部<span>{models.length}</span>
          </button>
          {vendors.map((v) => (
            <button
              key={v.name}
              type="button"
              role="radio"
              aria-checked={vendor === v.name}
              className={cn('cml-filter', vendor === v.name && 'is-selected')}
              data-testid={`cloud-models-vendor-${v.name}`}
              onClick={() => setVendor(vendor === v.name ? null : v.name)}
            >
              {v.name}
              <span>{v.count}</span>
            </button>
          ))}
        </div>
      )}
      {shown.length === 0 ? (
        <p className="cml-none" data-testid="cloud-models-no-match">
          没有匹配的模型
        </p>
      ) : (
        <div className="cml-grid">
          {shown.map((m) => (
            <ModelTile
              key={m.id}
              model={m}
              currency={catalog.currency}
              isDefault={m.id === catalog.defaultModel}
              current={currentId === `cloud:${m.id}`}
            />
          ))}
        </div>
      )}
    </div>
  );
}

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
              <div data-testid="cloud-models-error" className="flex items-center gap-2 px-4 py-3 text-[12px] text-warn">
                <span className="min-w-0 flex-1">云端模型目录加载失败:{error}</span>
                <SmBtn label={loading ? '重试中…' : '重试'} disabled={loading} onClick={() => void load()} />
              </div>
            ) : catalog === null ? (
              <div className="px-4 py-3 text-[12px] text-fg-3">加载中…</div>
            ) : models.length === 0 ? (
              <div data-testid="cloud-models-empty" className="px-4 py-3 text-[12px] text-fg-3">
                当前分组暂无可用模型
              </div>
            ) : (
              <CloudModelList catalog={catalog} />
            )}
          </SetCard>
        )}
      </div>
    </>
  );
}

/** 自定义模型(自带密钥)的折叠区:展开状态记在本机;云端禁用自带密钥(byoAllowed=false)时整块不渲染。 */
export function ByoAdvancedSection({ children }: { children: ReactNode }) {
  const byoAllowed = useAccountStore((st) => st.status?.byoAllowed !== false);
  if (!byoAllowed) return null;
  return (
    <AdvancedSection title="高级：自定义模型" testId="byo-advanced" storageKey="forge:settings.byoAdvancedOpen">
      {children}
    </AdvancedSection>
  );
}
