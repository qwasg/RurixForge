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
