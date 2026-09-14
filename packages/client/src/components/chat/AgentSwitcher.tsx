import { useEffect, useState } from 'react';
import { Bot, Cpu } from 'lucide-react';
import { getCodexAccount, getCodexStatus, type CodexStatus } from '@/lib/forgeApi';
import { useChatStore } from '@/lib/chatStore';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSettingsStore } from '@/lib/settingsStore';
import type { AgentEngine } from '@/lib/sessionStore';
import { cn } from '@/lib/cn';

function openCodexSettings() {
  useSettingsStore.getState().setPage('codex');
  useOverlayStore.getState().open('settings');
}

async function fetchCodexStatusWithAccount(): Promise<CodexStatus> {
  const status = await getCodexStatus();
  if (!status.installed || (typeof status.account?.authMode === 'string' && status.account.authMode !== '')) {
    return status;
  }
  const account = await getCodexAccount().catch(() => null);
  return account ? { ...status, account: { ...status.account, ...account } } : status;
}

/** 会话级本地/Codex 切换；不可用的 Codex 入口直接把人带到可解决问题的设置页。 */
export default function AgentSwitcher({
  engine,
  disabled = false,
  onPick,
}: {
  engine: AgentEngine;
  disabled?: boolean;
  onPick: (engine: AgentEngine) => void | Promise<void>;
}) {
  const [status, setStatus] = useState<CodexStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const observedAccount = useChatStore((state) => state.codexAccount);

  const loadStatus = async (): Promise<CodexStatus | null> => {
    try {
      const next = await fetchCodexStatusWithAccount();
      setStatus(next);
      useChatStore.setState({ codexAccount: next.account as Record<string, unknown> });
      if (
        next.installed &&
        typeof next.account?.authMode === 'string' &&
        next.account.authMode !== ''
      ) {
        await useChatStore.getState().ensureModels(true, 'codex');
      }
      return next;
    } catch {
      setStatus(null);
      return null;
    }
  };

  useEffect(() => {
    if (engine !== 'codex' || status !== null) return;
    let cancelled = false;
    void fetchCodexStatusWithAccount()
      .then((next) => {
        if (!cancelled) {
          setStatus(next);
          useChatStore.setState({ codexAccount: next.account as Record<string, unknown> });
          if (
            next.installed &&
            typeof next.account?.authMode === 'string' &&
            next.account.authMode !== ''
          ) {
            void useChatStore.getState().ensureModels(true, 'codex');
          }
        }
      })
      .catch(() => {
        if (!cancelled) setStatus(null);
      });
    return () => {
      cancelled = true;
    };
    // loadStatus 只在 Codex 首次呈现时运行；函数体没有组件外可变依赖。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [engine, status]);

  const installed = status?.installed === true;
  const account = observedAccount ?? status?.account;
  const authMode = account?.authMode;
  const ready = installed && typeof authMode === 'string' && authMode !== '';
  const plan = typeof account?.planType === 'string' ? account.planType : undefined;
  const codexHint = !status
    ? '检查 Codex 状态'
    : !installed
      ? '未安装 · 打开设置'
      : !ready
        ? '未登录 · 打开设置'
        : plan || '已登录';

  const pick = async (next: AgentEngine) => {
    if (disabled || checking || (next === engine && next !== 'codex')) return;
    setChecking(true);
    try {
      if (next === 'codex') {
        // 登录或安装可能刚在设置页完成；切换时必须重拉，不能信任旧缓存。
        const current = await loadStatus();
        const currentReady =
          current?.installed === true &&
          typeof current.account?.authMode === 'string' &&
          current.account.authMode !== '';
        if (!currentReady) {
          openCodexSettings();
          return;
        }
      }
      if (next !== engine) await onPick(next);
    } finally {
      setChecking(false);
    }
  };

  return (
    <div
      data-testid="agent-switcher"
      className="flex h-[22px] items-center overflow-hidden rounded-md border border-edge bg-shell-sunk"
    >
      <button
        type="button"
        data-testid="agent-engine-local"
        aria-pressed={engine === 'local'}
        disabled={disabled || checking}
        title="本地 Forge Agent"
        onClick={() => void pick('local')}
        className={cn(
          'flex h-full items-center gap-1 px-1.5 text-[10.5px] transition-colors',
          engine === 'local' ? 'bg-shell-panel text-fg' : 'text-fg-4 hover:bg-shell-hover',
          (disabled || checking) && 'cursor-not-allowed opacity-60',
        )}
      >
        <Cpu size={10} />
        本地
      </button>
      <button
        type="button"
        data-testid="agent-engine-codex"
        aria-pressed={engine === 'codex'}
        disabled={disabled || checking}
        title={codexHint}
        onClick={() => void pick('codex')}
        className={cn(
          'flex h-full items-center gap-1 border-l border-edge px-1.5 text-[10.5px] transition-colors',
          engine === 'codex' ? 'bg-acc-bg text-acc' : 'text-fg-4 hover:bg-shell-hover',
          (disabled || checking) && 'cursor-not-allowed opacity-60',
        )}
      >
        <Bot size={10} />
        Codex
        <span
          aria-hidden
          className={cn(
            'h-1.5 w-1.5 rounded-full',
            ready ? 'bg-dot-done' : installed ? 'bg-dot-running' : 'bg-dot-idle',
          )}
        />
        {engine === 'codex' && ready && plan && (
          <span data-testid="agent-engine-plan" className="max-w-[70px] truncate text-[9.5px] text-fg-3">
            {plan}
          </span>
        )}
      </button>
    </div>
  );
}
