import { useState } from 'react';
import { Pause, Play, Target, Trash2 } from 'lucide-react';
import { useGoalStore } from '@/lib/goalStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

function duration(seconds: number): string {
  if (seconds < 60) return `${Math.max(0, Math.round(seconds))}s`;
  const mins = Math.floor(seconds / 60);
  const rest = Math.round(seconds % 60);
  return rest > 0 ? `${mins}m ${rest}s` : `${mins}m`;
}

const STATUS: Record<string, string> = {
  active: '进行中',
  paused: '已暂停',
  completed: '已完成',
  blocked: '受阻',
};

function statusLabel(status: string, budgetExhausted?: boolean): string {
  return budgetExhausted && status === 'paused' ? '预算/额度受限' : (STATUS[status] ?? status);
}

/** Composer 上方的目标进度条；本地与 Codex 共用同一套 GoalStore。 */
export default function GoalBar() {
  const sessionId = useSessionStore((state) => state.activeSessionId);
  const goal = useGoalStore((state) => state.goal);
  const pauseGoal = useGoalStore((state) => state.pauseGoal);
  const resumeGoal = useGoalStore((state) => state.resumeGoal);
  const clearGoal = useGoalStore((state) => state.clearGoal);
  const openTab = useWorkbenchStore((state) => state.openTab);
  const [busy, setBusy] = useState(false);

  const runAction = async (action: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    try {
      await action();
    } finally {
      setBusy(false);
    }
  };

  if (!goal || goal.sessionId !== sessionId) return null;
  const budget = goal.tokenBudget;
  const percent = budget > 0 ? Math.min(100, (goal.tokensUsed / budget) * 100) : 0;

  return (
    <div data-testid="goal-bar" className="overflow-hidden rounded-xl border border-edge bg-shell-panel">
      <div className="flex items-center gap-2 px-3 py-2">
        <button
          type="button"
          data-testid="goal-bar-open"
          title="打开目标页签"
          onClick={() => openTab('goal')}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-acc-bg text-acc">
            <Target size={11} />
          </span>
          <span className="min-w-0 flex-1 truncate text-[11.5px] font-medium text-fg-2">{goal.objective}</span>
          <span className="shrink-0 rounded-full bg-shell-sunk px-1.5 py-0.5 text-[9.5px] text-fg-4">
            {goal.engine === 'codex' ? 'Codex' : '本地'} · {statusLabel(goal.status, goal.budgetExhausted)}
          </span>
        </button>
        {goal.status === 'active' && (
          <button
            type="button"
            aria-label="暂停目标"
            data-testid="goal-bar-pause"
            disabled={busy}
            onClick={() => void runAction(pauseGoal)}
            className="flex h-5 w-5 items-center justify-center rounded text-fg-3 hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-50"
          >
            <Pause size={10} />
          </button>
        )}
        {goal.status === 'paused' && (
          <button
            type="button"
            aria-label="恢复目标"
            data-testid="goal-bar-resume"
            disabled={busy}
            onClick={() => void runAction(resumeGoal)}
            className="flex h-5 w-5 items-center justify-center rounded text-acc hover:bg-acc-bg disabled:cursor-not-allowed disabled:opacity-50"
          >
            <Play size={10} />
          </button>
        )}
        <button
          type="button"
          aria-label="清除目标"
          data-testid="goal-bar-clear"
          disabled={busy}
          onClick={() => void runAction(clearGoal)}
          className="flex h-5 w-5 items-center justify-center rounded text-fg-4 hover:bg-shell-hover hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <Trash2 size={10} />
        </button>
      </div>
      <div className="flex items-center gap-2 border-t border-edge px-3 py-1.5 text-[9.5px] text-fg-4">
        <span>{duration(goal.timeUsedSeconds)}</span>
        <span>·</span>
        <span className="font-code">
          {goal.tokensUsed.toLocaleString()}{budget > 0 ? ` / ${budget.toLocaleString()}` : ''} tokens
        </span>
        <span>·</span>
        <span>{goal.turns} turns</span>
        {budget > 0 && (
          <span className="ml-auto h-1 w-20 overflow-hidden rounded-full bg-shell-active">
            <span className="block h-full rounded-full bg-acc" style={{ width: `${percent}%` }} />
          </span>
        )}
      </div>
    </div>
  );
}
