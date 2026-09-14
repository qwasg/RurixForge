import { useEffect } from 'react';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSkillStore } from '@/lib/skillStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { SkillDirsEditor } from '@/components/workbench/SkillsTab';
import { SetCard, SetH1, SetRow, SetSectionLabel, SmBtn } from './controls';

/**
 * F11 wave.5 技能页(按 D-F11-E 收窄):设置页不再重复渲染技能清单——
 * 新建/编辑/校验/启停/删除全在工作台「Skill 管理」tab,一处事实源。
 * 这里只留两张卡:跳转入口 + 技能目录(extraDirs)配置(与 tab 共用 skillStore)。
 */
export default function SkillsPage() {
  const load = useSkillStore((st) => st.load);

  useEffect(() => {
    void load();
  }, [load]);

  const openManager = () => {
    useWorkbenchStore.getState().openTab('skills');
    useOverlayStore.getState().closeAll();
  };

  return (
    <div data-testid="settings-page-skills" className="flex flex-col">
      <SetH1>技能</SetH1>
      <SetCard testId="skills-card">
        <SetRow
          last
          title="Skill 管理"
          desc="技能的新建、SKILL.md 编辑、格式校验、启停与删除统一在工作台「Skill 管理」页；设置页不再重复列出清单，避免两处事实源。"
          testId="skills-manager-row"
          control={
            <SmBtn
              accent
              label="打开 Skill 管理"
              testId="skills-open-manager"
              onClick={openManager}
            />
          }
        />
      </SetCard>
      <SetSectionLabel>技能目录</SetSectionLabel>
      <SetCard testId="skills-dirs-card">
        <div className="p-4">
          <SkillDirsEditor />
        </div>
      </SetCard>
    </div>
  );
}
