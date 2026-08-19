import { useSettingsStore } from '@/lib/settingsStore';
import { SetCard, SetH1, SetRow, SetToggle } from './controls';

/**
 * F7 wave.5 Agent 页(参考 set_page_agents 适配):
 * - Ctrl+Enter 发送(forge:submitCtrl;Composer 消费——开启后 Ctrl+Enter 发送、Enter 换行);
 * - 执行权限信息行(如实静态:本仓 agent 无 permission-mode 后端面,工具全部自动执行;
 *   参考的 auto/plan/bypass 三态同步留 RD-F7-002)。
 *
 * 差异留痕:参考的排队消息/用量摘要/联网搜索/层级 ignore 等偏好本仓均无后端语义面,
 * 不落空开关(诚实);联网开关本仓会话模型 webSearchEnabled 恒 true 无消费面,同样不落。
 */
export default function AgentPage() {
  const submitCtrlEnter = useSettingsStore((st) => st.submitCtrlEnter);
  const setSubmitCtrlEnter = useSettingsStore((st) => st.setSubmitCtrlEnter);

  return (
    <div data-testid="settings-page-agent" className="flex flex-col">
      <SetH1>Agent</SetH1>
      <div className="flex flex-col gap-3">
        <SetCard>
          <SetRow
            title="Ctrl + Enter 发送"
            desc="启用后，Ctrl+Enter 发送，Enter 换行"
            last
            testId="submit-ctrl-row"
            control={
              <SetToggle on={submitCtrlEnter} testId="submit-ctrl-toggle" onChange={setSubmitCtrlEnter} />
            }
          />
        </SetCard>
        <SetCard testId="permission-card">
          <SetRow
            title="执行权限"
            desc="当前后端的会话工具权限(如实静态面)"
            last
            control={
              <span className="font-code text-[11px] text-fg-3" data-testid="permission-mode-value">
                bypass · 工具全部自动执行
              </span>
            }
          />
        </SetCard>
      </div>
    </div>
  );
}
