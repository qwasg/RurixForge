import { useCallback, useEffect, useState } from 'react';
import { apiGet, apiPost } from '@/lib/forgeApi';
import { useToastStore } from '@/lib/toastStore';
import { SetCard, SetH1, SetRow, SetToggle } from './controls';

/**
 * F7 wave.5 技能页(F3 面平移,参考设置「规则 · 技能 · 子 Agent」的技能部分):
 * GET /api/forge/skills/list → 行(name + description + enabled toggle);
 * toggle → POST /api/forge/skills/config/write {disabled:[...]} 全量写回(读-改-写面不变)。
 */
interface SkillItem {
  name: string;
  description?: string;
  enabled: boolean;
}

export default function SkillsPage() {
  const [skills, setSkills] = useState<SkillItem[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const r = await apiGet<{ skills: SkillItem[] }>('/api/forge/skills/list');
      setSkills(r.skills);
      setLoadError(null);
    } catch (err) {
      setLoadError(err instanceof Error ? err.message : String(err));
      setSkills([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const toggle = async (name: string, enabled: boolean) => {
    if (busy || !skills) return;
    const next = skills.map((s) => (s.name === name ? { ...s, enabled } : s));
    setSkills(next); // 乐观
    setBusy(true);
    try {
      const disabled = next.filter((s) => !s.enabled).map((s) => s.name);
      await apiPost('/api/forge/skills/config/write', { disabled });
    } catch (err) {
      setSkills(skills); // 回滚
      useToastStore.getState().push('error', `技能配置写回失败:${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-testid="settings-page-skills" className="flex flex-col">
      <SetH1>技能</SetH1>
      {loadError && (
        <div className="mb-3 text-[12px] text-fg-3" data-testid="skills-load-error">
          技能清单加载失败:{loadError}
        </div>
      )}
      <SetCard testId="skills-card">
        {skills === null && !loadError && <div className="p-4 text-[12px] text-fg-4">加载中…</div>}
        {skills !== null && skills.length === 0 && !loadError && (
          <div className="p-4 text-[12px] text-fg-4">未发现技能</div>
        )}
        {(skills ?? []).map((s, i, arr) => (
          <SetRow
            key={s.name}
            title={s.name}
            desc={s.description?.trim() ? s.description : '（无描述）'}
            last={i === arr.length - 1}
            testId={`skill-row-${s.name}`}
            control={
              <SetToggle
                on={s.enabled}
                testId={`skill-toggle-${s.name}`}
                onChange={(v) => void toggle(s.name, v)}
              />
            }
          />
        ))}
      </SetCard>
    </div>
  );
}
