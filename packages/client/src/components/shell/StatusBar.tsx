import { useEffect, useState } from 'react';
import { MessageSquareText, PanelBottom, PanelLeft, PanelRight } from 'lucide-react';
import { apiGet } from '@/lib/forgeApi';
import { cn } from '@/lib/cn';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { StatusDot } from './primitives';

/**
 * F7 wave.3 StatusBar(26px,参考 ui/statusbar.rs):
 * 左 ● host 在线/离线(轮询 /api/forge/health 5s)+ provider 段(design-snapshot
 * models:deepseek available→「Live · deepseek-chat」,否则「Mock provider」)
 * + accent 段「{会话标题} · {todos done}/{total}」(无会话不显示);右 面板开关钮。
 */

interface SnapshotModel {
  id: string;
  label: string;
  provider: string;
  availability: string;
}
interface SnapshotTodo {
  status: string;
}
interface DesignSnapshot {
  models?: { models?: SnapshotModel[] };
  todos?: SnapshotTodo[];
}

const POLL_MS = 5000;

export default function StatusBar() {
  const activeSessionId = useSessionStore((st) => st.activeSessionId);
  const activeSession = useSessionStore((st) => st.sessions.find((s) => s.id === st.activeSessionId));
  const collapsed = useWorkbenchStore((st) => st.collapsed);
  const togglePane = useWorkbenchStore((st) => st.togglePane);
  const bottomOpen = useWorkbenchStore((st) => st.bottomOpen);
  const toggleBottom = useWorkbenchStore((st) => st.toggleBottom);

  const [online, setOnline] = useState(false);
  const [provider, setProvider] = useState<{ live: boolean; label: string }>({
    live: false,
    label: 'Mock provider',
  });
  const [todos, setTodos] = useState<{ done: number; total: number } | null>(null);

  useEffect(() => {
    let stop = false;
    const poll = async () => {
      try {
        await apiGet('/api/forge/health');
        if (!stop) setOnline(true);
      } catch {
        if (!stop) setOnline(false);
      }
      try {
        const q = activeSessionId ? `?sessionId=${encodeURIComponent(activeSessionId)}` : '';
        const snap = await apiGet<DesignSnapshot>(`/api/forge/design-snapshot${q}`);
        if (stop) return;
        const deepseek = snap.models?.models?.find((m) => m.provider === 'deepseek');
        if (deepseek && deepseek.availability === 'available') {
          setProvider({ live: true, label: `Live · ${deepseek.id}` });
        } else {
          setProvider({ live: false, label: 'Mock provider' });
        }
        const list = snap.todos ?? [];
        setTodos({
          done: list.filter((t) => t.status === 'completed' || t.status === 'done').length,
          total: list.length,
        });
      } catch {
        if (!stop) setTodos(null);
      }
    };
    void poll();
    const t = setInterval(() => void poll(), POLL_MS);
    return () => {
      stop = true;
      clearInterval(t);
    };
  }, [activeSessionId]);

  const seg = 'flex h-full items-center gap-[5px] px-1.5';
  const paneBtn = (on: boolean) =>
    cn(
      'flex h-full items-center px-1.5 text-fg-3 transition-colors hover:text-fg',
      on && 'text-fg',
    );

  return (
    <footer
      data-testid="shell-statusbar"
      className="flex h-[26px] shrink-0 items-center border-t border-edge bg-shell-sunk px-2 text-[11px] text-fg-3"
    >
      <span className={seg} data-testid="statusbar-host">
        <StatusDot color={online ? 'var(--dot-done)' : 'var(--dot-blocked)'} />
        {online ? 'host 在线' : 'host 离线'}
      </span>
      <span className={seg} data-testid="statusbar-provider">
        <StatusDot color={provider.live ? 'var(--dot-done)' : 'var(--dot-running)'} />
        {provider.label}
      </span>
      {activeSession && (
        <span className={cn(seg, 'text-acc')} data-testid="statusbar-session">
          <StatusDot color="var(--dot-running)" />
          {`${activeSession.title === '' ? activeSession.id : activeSession.title} · ${todos?.done ?? 0}/${todos?.total ?? 0}`}
        </span>
      )}
      <span className="flex-1" />
      <button
        type="button"
        title="切换底部面板(Ctrl+J)"
        aria-label="切换底部面板"
        data-testid="statusbar-bottom-toggle"
        className={paneBtn(bottomOpen)}
        onClick={toggleBottom}
      >
        <PanelBottom size={11} />
      </button>
      <button
        type="button"
        title="切换会话栏"
        aria-label="切换会话栏"
        className={paneBtn(!collapsed.sessions)}
        onClick={() => togglePane('sessions')}
      >
        <PanelLeft size={11} />
      </button>
      <button
        type="button"
        title="切换对话栏"
        aria-label="切换对话栏"
        className={paneBtn(!collapsed.chat)}
        onClick={() => togglePane('chat')}
      >
        <MessageSquareText size={11} />
      </button>
      <button
        type="button"
        title="切换 Inspector"
        aria-label="切换 Inspector"
        className={paneBtn(!collapsed.inspector)}
        onClick={() => togglePane('inspector')}
      >
        <PanelRight size={11} />
      </button>
    </footer>
  );
}
