/**
 * 设置页(07 §7.1 照搬 cindy 模式):左侧菜单 260px + 右侧内容区;
 * tab 切换经 URL ?tab= 深链;tab 单一事实源 = lib/forgeSettingsTabs.ts。
 * F3 落地范围:骨架 + skills tab 真实功能;其余 tab 占位并如实标注承接里程碑。
 */
import { useCallback, useEffect, useState } from 'react';
import { apiGet, apiPost } from '@/lib/forgeApi';
import {
  DEFAULT_TAB,
  TAB_IDS,
  TAB_LABELS,
  TAB_Landed,
  isSettingsTab,
  type SettingsTab,
} from '@/lib/forgeSettingsTabs';
import { cn } from '@/lib/cn';

interface SkillItem {
  name: string;
  description: string;
  enabled: boolean;
}

/** gen 后端清单条目(GET /api/forge/gen/backends;密钥值永不在此面,R-5)。 */
interface GenBackendItem {
  id: string;
  kind: string;
  configured: boolean;
  endpointSet: boolean;
  capabilities?: Record<string, unknown>;
}

/** 从 URL 读 ?tab=(深链直达;非法值回落 DEFAULT_TAB) */
function tabFromLocation(): SettingsTab {
  const q = new URLSearchParams(window.location.search).get('tab');
  return isSettingsTab(q) ? q : DEFAULT_TAB;
}

/** skills tab(07 §7.2):skill 列表 + 启用/禁用(写 config 立即生效)。 */
function SkillsSection() {
  const [skills, setSkills] = useState<SkillItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const r = await apiGet<{ skills: SkillItem[] }>('/api/forge/skills/list');
      setSkills(r.skills);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const toggle = async (name: string, enabled: boolean) => {
    setBusy(name);
    try {
      const disabled = skills.filter((s) => (s.name === name ? !enabled : !s.enabled)).map((s) => s.name);
      await apiPost('/api/forge/skills/config/write', { disabled });
      await load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  return (
    <section aria-label="skills-settings" className="space-y-2">
      <h2 className="text-sm font-medium text-ink">Skills</h2>
      <p className="text-xs text-muted">
        技能列表来自 /api/forge/skills/list 真实数据;禁用写入 data/skills-config.json 立即生效。
        seam 技能正文首行有依赖标注。
      </p>
      {error && <p className="text-xs text-red-600">{error}</p>}
      <ul className="divide-y divide-line-soft rounded-xl border border-line-soft bg-white">
        {skills.map((s) => (
          <li key={s.name} data-skill-name={s.name} className="flex items-center gap-3 px-3 py-2">
            <div className="min-w-0 flex-1">
              <div className="text-sm text-ink">{s.name}</div>
              <div className="truncate text-xs text-muted-faint">{s.description}</div>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={s.enabled}
              disabled={busy === s.name}
              onClick={() => void toggle(s.name, !s.enabled)}
              className={cn(
                'relative h-5 w-9 shrink-0 rounded-full transition-colors',
                s.enabled ? 'bg-ink' : 'bg-line',
                busy === s.name && 'opacity-50',
              )}
              title={s.enabled ? '禁用' : '启用'}
            >
              <span
                className={cn(
                  'absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all',
                  s.enabled ? 'left-4.5 left-[18px]' : 'left-0.5',
                )}
              />
            </button>
          </li>
        ))}
        {skills.length === 0 && !error && (
          <li className="px-3 py-2 text-xs text-muted-faint">加载中…</li>
        )}
      </ul>
    </section>
  );
}

/** generation tab(F5 wave.3):gen 后端清单 + 配置表单。
 * 数据源:GET /api/forge/gen/backends;配置:POST /api/forge/gen/backends/configure。
 * R-5:apiKey 只进请求体写本地 keystore,任何响应/回显不含密钥值。 */
function GenerationSection() {
  const [backends, setBackends] = useState<GenBackendItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string>('');
  const [enabled, setEnabled] = useState(false);
  const [endpoint, setEndpoint] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const r = await apiGet<{ backends: GenBackendItem[] }>('/api/forge/gen/backends');
      setBackends(r.backends);
      setError(null);
      setSelectedId((cur) => cur || r.backends[0]?.id || '');
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const selected = backends.find((b) => b.id === selectedId) ?? null;
  // 选中切换时表单基线 = 该后端当前配置面(enabled 真实;endpoint 值不出,只按 endpointSet 提示)。
  useEffect(() => {
    setEnabled(selected?.configured ?? false);
    setEndpoint('');
    setApiKey('');
  }, [selectedId, selected?.configured]);

  const submit = async () => {
    if (!selected || busy) return;
    setBusy(true);
    setNotice(null);
    try {
      const payload: Record<string, unknown> = {
        id: selected.id,
        kind: selected.kind,
        enabled,
      };
      if (endpoint.trim()) payload.endpoint = endpoint.trim();
      if (apiKey.trim()) payload.apiKey = apiKey.trim();
      const r = await apiPost<{ ok: boolean; configured: boolean }>(
        '/api/forge/gen/backends/configure',
        payload,
      );
      setNotice(`已保存:${selected.id} configured=${r.configured}`);
      setApiKey('');
      await load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section aria-label="generation-settings" className="space-y-3">
      <h2 className="text-sm font-medium text-ink">Generation</h2>
      <p className="text-xs text-muted">
        生成后端清单来自 /api/forge/gen/backends 真实判定(configured = 配置完备可用)。
        密钥写入本地 keystore(data/keystore.json),不在此回显,也不进 gen-backends.json。
      </p>
      {error && <p className="text-xs text-red-600">{error}</p>}
      {notice && <p className="text-xs text-accent-green">{notice}</p>}

      <ul className="divide-y divide-line-soft rounded-xl border border-line-soft bg-white">
        {backends.map((b) => (
          <li key={b.id} data-gen-backend-row={b.id} className="flex items-center gap-3 px-3 py-2">
            <div className="min-w-0 flex-1">
              <div className="text-sm text-ink">{b.id}</div>
              <div className="truncate text-xs text-muted-faint">
                kind={b.kind} · endpointSet={b.endpointSet ? 'true' : 'false'}
              </div>
            </div>
            <span
              className={cn(
                'rounded-full px-2 py-0.5 text-2xs',
                b.configured ? 'bg-accent-green text-white' : 'bg-panel text-muted',
              )}
            >
              {b.configured ? 'configured' : '未配置'}
            </span>
          </li>
        ))}
        {backends.length === 0 && !error && (
          <li className="px-3 py-2 text-xs text-muted-faint">加载中…</li>
        )}
      </ul>

      {selected && (
        <div className="space-y-2 rounded-xl border border-line-soft bg-white p-3" data-gen-config-form>
          <div className="flex items-center gap-2">
            <label className="text-xs text-muted">后端</label>
            <select
              data-gen-select
              value={selectedId}
              onChange={(e) => setSelectedId(e.target.value)}
              className="rounded-md border border-line bg-white px-1.5 py-1 text-xs text-ink outline-none"
            >
              {backends.map((b) => (
                <option key={b.id} value={b.id}>
                  {b.id}
                </option>
              ))}
            </select>
            <label className="ml-2 flex items-center gap-1 text-xs text-muted">
              <input
                type="checkbox"
                data-gen-enabled
                checked={enabled}
                onChange={(e) => setEnabled(e.target.checked)}
              />
              enabled
            </label>
          </div>
          {selected.kind === 'remote' && (
            <div>
              <label className="block text-2xs text-muted">
                endpoint{selected.endpointSet ? '(已设置,留空保持不变)' : '(remote 必填)'}
              </label>
              <input
                data-gen-endpoint
                value={endpoint}
                onChange={(e) => setEndpoint(e.target.value)}
                placeholder="https://api.example.com"
                className="mt-0.5 w-full rounded-md border border-line bg-white px-2 py-1 text-xs text-ink outline-none placeholder:text-muted-faint"
              />
            </div>
          )}
          <div>
            <label className="block text-2xs text-muted">apiKey(remote 需要)</label>
            <input
              type="password"
              data-gen-apikey
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder="留空保持不变"
              className="mt-0.5 w-full rounded-md border border-line bg-white px-2 py-1 text-xs text-ink outline-none placeholder:text-muted-faint"
            />
          </div>
          <div className="flex justify-end">
            <button
              type="button"
              data-gen-save
              disabled={busy}
              onClick={() => void submit()}
              className={cn(
                'rounded-md px-3 py-1 text-xs text-white',
                busy ? 'cursor-not-allowed bg-muted-faint' : 'bg-ink',
              )}
            >
              {busy ? '保存中…' : '保存配置'}
            </button>
          </div>
        </div>
      )}
    </section>
  );
}

export default function SettingsView() {
  const [tab, setTab] = useState<SettingsTab>(tabFromLocation);

  // URL 深链:?tab=xxx 双向同步(replaceState,不产生历史堆栈)。
  useEffect(() => {
    const url = new URL(window.location.href);
    url.searchParams.set('tab', tab);
    window.history.replaceState(null, '', url.toString());
  }, [tab]);

  useEffect(() => {
    const onPop = () => setTab(tabFromLocation());
    window.addEventListener('popstate', onPop);
    return () => window.removeEventListener('popstate', onPop);
  }, []);

  return (
    <div className="flex h-full" data-testid="settings-view">
      {/* 左侧菜单(260px,cindy DEFAULT_SETTINGS_MENU_WIDTH 对齐) */}
      <nav className="w-[260px] shrink-0 space-y-0.5 overflow-y-auto border-r border-line-soft bg-panel px-2 py-3">
        {TAB_IDS.map((id) => (
          <button
            key={id}
            type="button"
            data-tab-id={id}
            onClick={() => setTab(id)}
            className={cn(
              'flex w-full items-center justify-between rounded-lg px-3 py-1.5 text-left text-sm transition-colors',
              tab === id ? 'bg-white font-medium text-ink shadow-composer' : 'text-ink-soft hover:bg-panel-hover',
            )}
          >
            <span>{TAB_LABELS[id]}</span>
            {TAB_Landed[id] === null && (
              <span className="text-2xs text-muted-faint">后续</span>
            )}
          </button>
        ))}
      </nav>
      {/* 右侧内容区 */}
      <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
        {tab === 'skills' ? (
          <SkillsSection />
        ) : tab === 'generation' ? (
          <GenerationSection />
        ) : (
          <section className="space-y-2">
            <h2 className="text-sm font-medium text-ink">{TAB_LABELS[tab]}</h2>
            <p className="text-xs text-muted">
              此 tab 未落地{TAB_Landed[tab] ? `(承接:${TAB_Landed[tab]})` : '(后续里程碑)'}
              ——如实占位,不伪造功能(07 §7.2 首发冻结清单)。
            </p>
          </section>
        )}
      </main>
    </div>
  );
}
