import { useEffect, useState } from 'react';
import { apiGet } from '@/lib/forgeApi';
import { cn } from '@/lib/cn';
import { useSessionStore } from '@/lib/sessionStore';
import { useChatStore } from '@/lib/chatStore';
import { StatusDot } from './primitives';

/**
 * F7 wave.3 StatusBar(26px,参考 ui/statusbar.rs):
 * 左 ● host 在线/离线(轮询 /api/forge/health 5s)+ provider 段(当前会话选中模型,
 * 否则 defaultModelId;可用非 mock →「Live · {label}」,否则「Mock provider」)
 * + accent 段「{会话标题} · {todos done}/{total}」(无会话不显示)。
 * 面板开关钮已上移至 TitleBar。
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
/** F-GAME-3:项目面(2D/3D 模式徽标事实源) */
interface SnapshotProject {
  name: string;
  mode: string;
  root?: string;
}
interface DesignSnapshot {
  models?: { models?: SnapshotModel[]; defaultModelId?: string };
  todos?: SnapshotTodo[];
  project?: SnapshotProject;
  activeSession?: { selectedModelId?: string | null };
}

const POLL_MS = 5000;

export default function StatusBar() {
  const activeSessionId = useSessionStore((st) => st.activeSessionId);
  const activeSession = useSessionStore((st) => st.sessions.find((s) => s.id === st.activeSessionId));

  const [online, setOnline] = useState(false);
  const [provider, setProvider] = useState<{ live: boolean; label: string }>({
    live: false,
    label: 'Mock provider',
  });
  const [todos, setTodos] = useState<{ done: number; total: number } | null>(null);
  const [project, setProject] = useState<SnapshotProject | null>(null);

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
        if (snap.models?.models && snap.models.models.length > 0) {
          if (useChatStore.getState().models.length === 0) {
            useChatStore.setState({
              models: snap.models.models,
              defaultModelId: snap.models.defaultModelId ?? useChatStore.getState().defaultModelId,
            });
          }
        }
        const catalog = snap.models?.models ?? [];
        const wanted =
          snap.activeSession?.selectedModelId ?? snap.models?.defaultModelId ?? null;
        const chosen =
          catalog.find((m) => m.id === wanted) ??
          catalog.find((m) => m.provider !== 'mock' && m.availability === 'available') ??
          catalog.find((m) => m.provider === 'mock');
        if (chosen && chosen.provider !== 'mock' && chosen.availability === 'available') {
          setProvider({ live: true, label: `Live · ${chosen.label || chosen.id}` });
        } else {
          setProvider({ live: false, label: 'Mock provider' });
        }
        const list = snap.todos ?? [];
        setTodos({
          done: list.filter((t) => t.status === 'completed' || t.status === 'done').length,
          total: list.length,
        });
        setProject(snap.project ?? null);
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
      {/* F-GAME-3:当前项目 2D/3D 模式徽标(forge.toml 事实源,5s 轮询) */}
      {project && (
        <span
          className={seg}
          data-testid="statusbar-project-mode"
          title={`${project.name} · forge.toml mode=${project.mode}`}
        >
          <StatusDot color={project.mode === '2d' ? 'var(--dot-done)' : 'var(--dot-running)'} />
          {`${project.name} · ${project.mode === '2d' ? '2D' : '3D'}`}
        </span>
      )}
      <span className="flex-1" />
    </footer>
  );
}
