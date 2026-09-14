import { useEffect, useState } from 'react';
import { Pause, Play, Save, Target, Trash2 } from 'lucide-react';
import { useGoalStore } from '@/lib/goalStore';
import { useSessionStore } from '@/lib/sessionStore';
import { cn } from '@/lib/cn';

const STATUS: Record<string, string> = {
  active: '进行中',
  paused: '已暂停',
  completed: '已完成',
  blocked: '受阻',
  cleared: '已清除',
};

function statusLabel(status: string, budgetExhausted?: boolean): string {
  return budgetExhausted && status === 'paused' ? '预算/额度受限' : (STATUS[status] ?? status);
}

function dateLabel(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString('zh-CN');
}

/** 两种引擎共用的目标编辑、预算与状态时间线。 */
export default function GoalTab() {
  const sessionId = useSessionStore((state) => state.activeSessionId);
  const goal = useGoalStore((state) => state.goal);
  const loadedSessionId = useGoalStore((state) => state.sessionId);
  const history = useGoalStore((state) => state.history);
  const loading = useGoalStore((state) => state.loading);
  const error = useGoalStore((state) => state.error);
  const loadGoal = useGoalStore((state) => state.loadGoal);
  const setGoal = useGoalStore((state) => state.setGoal);
  const pauseGoal = useGoalStore((state) => state.pauseGoal);
  const resumeGoal = useGoalStore((state) => state.resumeGoal);
  const clearGoal = useGoalStore((state) => state.clearGoal);
  const [objective, setObjective] = useState('');
  const [budget, setBudget] = useState('100000');
  const [saving, setSaving] = useState(false);
  const [acting, setActing] = useState(false);

  useEffect(() => {
    if (loadedSessionId !== sessionId) {
      // 不让上一会话的草稿在新会话加载期间（或无目标时）留在表单里。
      setObjective('');
      setBudget('100000');
      void loadGoal(sessionId);
    }
  }, [loadGoal, loadedSessionId, sessionId]);

  useEffect(() => {
    if (goal && goal.sessionId === sessionId) {
      setObjective(goal.objective);
      setBudget(goal.tokenBudget > 0 ? String(goal.tokenBudget) : '');
      return;
    }
    if (!loading && loadedSessionId === sessionId) {
      setObjective('');
      setBudget('100000');
    }
  }, [goal, loadedSessionId, loading, sessionId]);

  const save = async () => {
    const value = objective.trim();
    if (!sessionId || value === '') return;
    const parsed = Number(budget);
    setSaving(true);
    try {
      await setGoal(value, Number.isFinite(parsed) && parsed > 0 ? Math.round(parsed) : undefined);
    } finally {
      setSaving(false);
    }
  };

  const runAction = async (action: () => Promise<unknown>) => {
    if (acting || saving || loading) return;
    setActing(true);
    try {
      await action();
    } finally {
      setActing(false);
    }
  };

  const controlsBusy = loading || saving || acting;

  if (!sessionId) {
    return (
      <div data-testid="goal-tab" className="flex h-full items-center justify-center text-[12px] text-fg-4">
        先选择或创建会话，再设置目标。
      </div>
    );
  }

  return (
    <div data-testid="goal-tab" className="flex h-full min-h-0 flex-col overflow-y-auto bg-shell-bg px-6 py-5">
      <div className="mx-auto flex w-full max-w-[760px] flex-col gap-4">
        <div className="flex items-start gap-3">
          <span className="mt-0.5 flex h-8 w-8 items-center justify-center rounded-lg bg-acc-bg text-acc"><Target size={16} /></span>
          <div className="min-w-0 flex-1">
            <h1 className="font-serif text-[24px] font-bold text-fg">目标</h1>
            <p className="mt-0.5 text-[11.5px] text-fg-3">Agent 会持续推进，直到完成、受阻、暂停或耗尽预算。</p>
          </div>
          {goal && <span className="rounded-full bg-shell-sunk px-2 py-1 text-[10px] text-fg-3">{goal.engine === 'codex' ? 'Codex' : '本地'} · {statusLabel(goal.status, goal.budgetExhausted)}</span>}
        </div>

        {error && <div className="rounded-lg border border-edge bg-warn-bg px-3 py-2 text-[11px] text-warn">{error}</div>}
        <section className="overflow-hidden rounded-[10px] border border-edge bg-shell-sunk">
          <label className="flex flex-col gap-1.5 px-4 py-3.5">
            <span className="text-[12px] font-medium text-fg">目标内容</span>
            <textarea
              data-testid="goal-objective"
              value={objective}
              onChange={(event) => setObjective(event.target.value)}
              placeholder="描述需要持续推进到完成的结果…"
              className="min-h-[92px] resize-y rounded-lg border border-edge bg-shell-panel px-3 py-2 text-[12.5px] leading-[19px] text-fg outline-none placeholder:text-fg-4 focus:border-acc-ring"
            />
          </label>
          <div className="flex items-center gap-4 border-t border-edge px-4 py-3">
            <label className="flex min-w-0 flex-1 flex-col gap-1">
              <span className="text-[11.5px] font-medium text-fg-2">Token 预算</span>
              <span className="text-[10.5px] text-fg-4">留空使用服务端默认预算</span>
            </label>
            <input
              data-testid="goal-budget"
              type="number"
              min={1}
              step={1000}
              value={budget}
              onChange={(event) => setBudget(event.target.value)}
              className="w-[150px] rounded-md border border-edge bg-shell-panel px-2 py-1.5 text-right font-code text-[11px] text-fg outline-none focus:border-acc-ring"
            />
          </div>
          <div className="flex flex-wrap justify-end gap-1.5 border-t border-edge px-4 py-3">
            {goal?.status === 'active' && (
              <button type="button" data-testid="goal-pause" disabled={controlsBusy} onClick={() => void runAction(pauseGoal)} className="flex h-[27px] items-center gap-1 rounded-md border border-edge bg-shell-panel px-2.5 text-[11px] text-fg-2 hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-50"><Pause size={11} />暂停</button>
            )}
            {goal?.status === 'paused' && (
              <button type="button" data-testid="goal-resume" disabled={controlsBusy} onClick={() => void runAction(resumeGoal)} className="flex h-[27px] items-center gap-1 rounded-md border border-edge bg-shell-panel px-2.5 text-[11px] text-fg-2 hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-50"><Play size={11} />恢复</button>
            )}
            {goal && (
              <button type="button" data-testid="goal-clear" disabled={controlsBusy} onClick={() => void runAction(clearGoal)} className="flex h-[27px] items-center gap-1 rounded-md border border-edge px-2.5 text-[11px] text-fg-3 hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-50"><Trash2 size={11} />清除</button>
            )}
            <button
              type="button"
              data-testid="goal-save"
              disabled={controlsBusy || objective.trim() === ''}
              onClick={() => void save()}
              className={cn('flex h-[27px] items-center gap-1 rounded-md border px-2.5 text-[11px]', controlsBusy || objective.trim() === '' ? 'cursor-not-allowed border-edge bg-shell-panel text-fg-4' : 'border-acc bg-acc text-fg-inv hover:bg-acc-soft')}
            >
              <Save size={11} />{saving ? '保存中…' : goal ? '更新目标' : '开始目标'}
            </button>
          </div>
        </section>

        {goal && (
          <section data-testid="goal-metrics" className="grid grid-cols-3 overflow-hidden rounded-[10px] border border-edge bg-shell-sunk max-[560px]:grid-cols-1">
            <div className="px-4 py-3"><div className="text-[10px] text-fg-4">Tokens</div><div className="mt-1 font-code text-[14px] text-fg-2">{goal.tokensUsed.toLocaleString()}<span className="text-[10px] text-fg-4"> / {goal.tokenBudget > 0 ? goal.tokenBudget.toLocaleString() : '—'}</span></div></div>
            <div className="border-l border-edge px-4 py-3 max-[560px]:border-l-0 max-[560px]:border-t"><div className="text-[10px] text-fg-4">用时</div><div className="mt-1 font-code text-[14px] text-fg-2">{Math.round(goal.timeUsedSeconds)}s</div></div>
            <div className="border-l border-edge px-4 py-3 max-[560px]:border-l-0 max-[560px]:border-t"><div className="text-[10px] text-fg-4">轮次</div><div className="mt-1 font-code text-[14px] text-fg-2">{goal.turns}</div></div>
          </section>
        )}

        <section className="flex flex-col gap-2">
          <h2 className="text-[11px] font-semibold text-fg-3">状态时间线</h2>
          {history.length === 0 ? (
            <div className="text-[11px] text-fg-4">尚无状态记录。</div>
          ) : (
            <div className="flex flex-col border-l border-edge pl-3">
              {[...history].reverse().map((entry, index) => (
                <div key={`${entry.ts}:${index}`} className="relative flex gap-3 py-2">
                  <span className="absolute -left-[15.5px] top-[13px] h-[5px] w-[5px] rounded-full bg-dot-idle" />
                  <span className="w-[120px] shrink-0 text-[10px] text-fg-4">{dateLabel(entry.ts)}</span>
                  <span className="text-[11px] text-fg-2">{STATUS[entry.status] ?? entry.status}{entry.note ? ` · ${entry.note}` : ''}</span>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
