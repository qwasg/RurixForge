import { useEffect, useState } from 'react';
import { apiGet, apiPatch } from '@/lib/forgeApi';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useToastStore } from '@/lib/toastStore';
import { SetCard, SetH1, SetRow, SetToggle } from './controls';

const MODES: Array<{ id: 'bypass' | 'plan' | 'auto'; label: string; desc: string }> = [
  { id: 'bypass', label: 'bypass', desc: '工具全部自动执行' },
  { id: 'plan', label: 'plan', desc: '仅允许只读工具' },
  { id: 'auto', label: 'auto', desc: '写操作需批准' },
];

function modeCaption(mode: string): string {
  if (mode === 'plan') return 'plan · 仅允许只读工具';
  if (mode === 'auto') return 'auto · 写操作需批准';
  return 'bypass · 工具全部自动执行';
}

/**
 * Agent 页:Ctrl+Enter 发送 + 执行权限三态(bypass/plan/auto)。
 */
export default function AgentPage() {
  const submitCtrlEnter = useSettingsStore((st) => st.submitCtrlEnter);
  const setSubmitCtrlEnter = useSettingsStore((st) => st.setSubmitCtrlEnter);
  const sessionId = useSessionStore((st) => st.activeSessionId);
  const pending = useChatStore((st) => st.pendingPermission);
  const resolvePermission = useChatStore((st) => st.resolvePermission);
  const [mode, setMode] = useState('bypass');

  useEffect(() => {
    if (!sessionId) {
      setMode('bypass');
      return;
    }
    let cancelled = false;
    void apiGet<{ mode?: string }>(`/api/forge/sessions/${encodeURIComponent(sessionId)}/permission`)
      .then((r) => {
        if (!cancelled && typeof r.mode === 'string') setMode(r.mode);
      })
      .catch(() => {
        /* 无会话权限面时保持 bypass */
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  const pickMode = async (next: string) => {
    if (!sessionId) {
      useToastStore.getState().push('warning', '请先选择会话');
      return;
    }
    const prev = mode;
    setMode(next);
    try {
      const r = await apiPatch<{ mode?: string }>(
        `/api/forge/sessions/${encodeURIComponent(sessionId)}/permission`,
        { mode: next },
      );
      if (typeof r.mode === 'string') setMode(r.mode);
    } catch (err) {
      setMode(prev);
      const msg = err instanceof Error ? err.message : String(err);
      useToastStore.getState().push('error', `切换权限失败:${msg}`);
    }
  };

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
            desc="当前会话的工具权限模式"
            last
            control={
              <span className="font-code text-[11px] text-fg-3" data-testid="permission-mode-value">
                {modeCaption(mode)}
              </span>
            }
          />
          <div className="flex flex-wrap gap-1.5 border-t border-edge px-4 py-3">
            {MODES.map((m) => (
              <button
                key={m.id}
                type="button"
                data-testid={`permission-mode-${m.id}`}
                onClick={() => void pickMode(m.id)}
                className={
                  m.id === mode
                    ? 'rounded-md bg-acc-bg px-2 py-1 text-[11px] text-acc'
                    : 'rounded-md border border-edge px-2 py-1 text-[11px] text-fg-3 hover:bg-shell-hover'
                }
              >
                {m.label}
                <span className="ml-1 text-fg-4">{m.desc}</span>
              </button>
            ))}
          </div>
          {pending && (
            <div className="flex items-center gap-2 border-t border-edge px-4 py-3" data-testid="permission-pending">
              <span className="min-w-0 flex-1 text-[12px] text-fg-2">批准工具 {pending.tool}？</span>
              <button
                type="button"
                data-testid="permission-approve"
                className="rounded-md bg-acc px-2 py-1 text-[11px] text-fg-inv"
                onClick={() => void resolvePermission(true)}
              >
                批准
              </button>
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
