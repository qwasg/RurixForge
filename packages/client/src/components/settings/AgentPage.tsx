import { useEffect, useRef, useState } from 'react';
import {
  getAgentConfig, patchAgentConfig, postCodexConfig,
  type AgentConfigFace, type AgentPermissionMode,
} from '@/lib/forgeApi';
import { useChatStore } from '@/lib/chatStore';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useToastStore } from '@/lib/toastStore';
import { SetCard, SetH1, SetRow, SetSelect, SetToggle } from './controls';

/**
 * Agent defaults are persisted by agentd; options come from its capabilities.
 */
export default function AgentPage() {
  const submitCtrlEnter = useSettingsStore((st) => st.submitCtrlEnter);
  const setSubmitCtrlEnter = useSettingsStore((st) => st.setSubmitCtrlEnter);
  const defaultAgentEngine = useSessionStore((st) => st.defaultAgentEngine);
  const hydrateAgentDefaults = useSessionStore((st) => st.hydrateAgentDefaults);
  const pending = useChatStore((st) => st.pendingPermission);
  const resolvePermission = useChatStore((st) => st.resolvePermission);
  const [settings, setSettings] = useState<AgentConfigFace | null>(null);
  const [settingsLoading, setSettingsLoading] = useState(true);
  const [settingsSaving, setSettingsSaving] = useState(false);
  const [settingsError, setSettingsError] = useState('');
  const [reload, setReload] = useState(0);
  const [engineSaving, setEngineSaving] = useState(false);
  const configVersion = useRef(0);
  const models = settings?.options.exploreModels ?? [];
  const savedModel = settings?.config.exploreModel ?? '';
  const modelOptions = [
    { value: '', label: '自动 · 沿用档案或父会话模型' },
    ...models.map((model) => ({
      value: model.id,
      label: `${model.group} · ${model.label}${model.availability === 'available' ? '' : '（未就绪）'}`,
      disabled: model.availability !== 'available',
    })),
    ...(savedModel && !models.some((model) => model.id === savedModel)
      ? [{ value: savedModel, label: `${savedModel}（当前不可用）`, disabled: true }] : []),
  ];
  const detailedApproval = pending
    ? pending.approvalKind !== 'command' || Boolean(
        pending.command || pending.cwd || pending.changes?.length || pending.reason ||
        pending.questions?.length || pending.permissions || pending.schema || pending.grantRoot ||
        pending.networkApprovalContext || pending.proposedExecpolicyAmendment ||
        pending.proposedNetworkPolicyAmendments || pending.url,
      )
    : false;

  useEffect(() => {
    const version = ++configVersion.current;
    setSettingsLoading(true);
    setSettingsSaving(false);
    setSettingsError('');
    void getAgentConfig().then((result) => {
      if (version === configVersion.current) setSettings(result);
    }).catch((err: unknown) => {
      if (version === configVersion.current) setSettingsError(err instanceof Error ? err.message : String(err));
    }).finally(() => {
      if (version === configVersion.current) setSettingsLoading(false);
    });
    return () => { configVersion.current++; };
  }, [reload]);


  const saveDefaults = async (patch: Partial<AgentConfigFace['config']>) => {
    if (!settings || settingsLoading || settingsSaving) return;
    const version = configVersion.current;
    setSettingsSaving(true);
    try {
      const result = await patchAgentConfig(patch);
      if (version === configVersion.current) setSettings(result);
    } catch (err) {
      if (version === configVersion.current) useToastStore.getState().push('error', `保存 Agent 设置失败:${err instanceof Error ? err.message : String(err)}`);
    } finally {
      if (version === configVersion.current) setSettingsSaving(false);
    }
  };


  const pickDefaultEngine = async (next: 'local' | 'codex') => {
    if (engineSaving) return;
    setEngineSaving(true);
    try {
      const result = await postCodexConfig({ defaultEngine: next });
      hydrateAgentDefaults(result.config?.defaultEngine ?? next);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      useToastStore.getState().push('error', `切换默认引擎失败:${msg}`);
    } finally {
      setEngineSaving(false);
    }
  };

  return (
    <div data-testid="settings-page-agent" className="flex flex-col">
      <SetH1>Agent</SetH1>
      <div className="flex flex-col gap-3">
        {settingsError && (
          <div role="alert" className="flex items-center gap-3 rounded-md border border-edge px-4 py-3 text-[12px] text-warn">
            <span className="min-w-0 flex-1">设置同步失败：{settingsError}</span>
            <button type="button" className="shrink-0 text-fg-2 underline" onClick={() => setReload((value) => value + 1)}>重试</button>
          </div>
        )}
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
        <SetCard testId="default-engine-card">
          <SetRow
            title="新会话默认引擎"
            desc="只影响之后创建的会话，已有会话可在输入框下方单独切换"
            control={
              <div className="flex items-center overflow-hidden rounded-md border border-edge bg-shell-panel">
                {(['local', 'codex'] as const).map((engine) => (
                  <button
                    key={engine}
                    type="button"
                    data-testid={`default-engine-${engine}`}
                    disabled={engineSaving}
                    aria-pressed={engine === defaultAgentEngine}
                    onClick={() => void pickDefaultEngine(engine)}
                    className={
                      engine === defaultAgentEngine
                        ? 'h-[26px] bg-acc-bg px-2.5 text-[11px] text-acc'
                        : 'h-[26px] border-l border-edge px-2.5 text-[11px] text-fg-3 first:border-l-0 hover:bg-shell-hover'
                    }
                  >
                    {engine === 'local' ? '本地' : 'Codex'}
                  </button>
                ))}
              </div>
            }
          />
          <SetRow
            title="Explore Agent 默认模型"
            desc="用于本地引擎的只读调研子代理。自动时沿用档案或父会话模型；档案中明确指定的模型优先。"
            last
            control={
              <SetSelect
                testId="agent-explore-model"
                ariaLabel="Explore Agent 默认模型"
                value={savedModel}
                options={modelOptions}
                disabled={!settings || settingsLoading || settingsSaving}
                onPick={(exploreModel) => void saveDefaults({ exploreModel })}
              />
            }
          />
        </SetCard>
        <SetCard testId="permission-card">
          <SetRow
            title="执行权限"
            desc="权限选项与后台一致，同时用于本地和 Codex 会话。Plan 与 Explore 的只读限制仍然生效。"
          />
          <SetRow
            title="新会话默认权限"
            desc="只影响之后创建的会话，已有会话保留各自的权限。"
            last
            control={
              <SetSelect
                testId="agent-default-permission"
                ariaLabel="新会话默认权限"
                value={settings?.config.defaultPermissionMode ?? ''}
                options={settings?.options.permissionModes.map((option) => ({ value: option.id, label: option.label })) ?? []}
                disabled={!settings || settingsLoading || settingsSaving}
                onPick={(defaultPermissionMode) => void saveDefaults({ defaultPermissionMode: defaultPermissionMode as AgentPermissionMode })}
              />
            }
          />
          <div aria-live="polite" className="px-4 pb-3 text-[10.5px] text-fg-4">
            {settingsLoading ? '正在读取后台设置…' : settingsSaving ? '正在保存…' : settings ? '设置保存在本机，重启后仍然生效。' : '后台设置尚未加载。'}
          </div>
          {pending && (
            <div className="flex items-center gap-2 border-t border-edge px-4 py-3" data-testid="permission-pending">
              <span className="min-w-0 flex-1 text-[12px] text-fg-2">
                {detailedApproval ? `工具 ${pending.tool} 正在等待详细审批` : `批准工具 ${pending.tool}？`}
              </span>
              {detailedApproval ? (
                <button
                  type="button"
                  data-testid="permission-review"
                  className="rounded-md bg-acc px-2 py-1 text-[11px] text-fg-inv"
                  onClick={() => useOverlayStore.getState().close('settings')}
                >
                  回到对话查看
                </button>
              ) : (
                <button
                  type="button"
                  data-testid="permission-approve"
                  className="rounded-md bg-acc px-2 py-1 text-[11px] text-fg-inv"
                  onClick={() => void resolvePermission(true)}
                >
                  批准
                </button>
              )}
              <button
                type="button"
                data-testid="permission-deny"
                className="rounded-md border border-edge px-2 py-1 text-[11px] text-fg-3"
                onClick={() => void resolvePermission(false)}
              >
                拒绝
              </button>
            </div>
          )}
        </SetCard>
      </div>
    </div>
  );
}
