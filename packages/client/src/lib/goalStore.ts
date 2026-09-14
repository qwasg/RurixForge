import { create } from 'zustand';
import {
  deleteGoal,
  getGoal,
  postGoalPause,
  postGoalResume,
  putGoal,
  type GoalFace,
} from './forgeApi';
import { useSessionStore } from './sessionStore';
import { useToastStore } from './toastStore';
import { useWorkbenchStore } from './workbenchStore';

export type GoalStatus = 'active' | 'paused' | 'completed' | 'blocked';

export interface Goal extends Omit<GoalFace, 'sessionId' | 'status' | 'engine' | 'createdAt' | 'updatedAt'> {
  sessionId: string;
  status: GoalStatus;
  engine: 'local' | 'codex';
  tokenBudget: number;
  tokensUsed: number;
  timeUsedSeconds: number;
  turns: number;
  createdAt?: string;
  updatedAt?: string;
}

export interface GoalHistoryEntry {
  status: GoalStatus | 'cleared';
  ts: string;
  note?: string | null;
  tokensUsed?: number;
  timeUsedSeconds?: number;
}

export interface GoalEventLike {
  sessionId?: string;
  type: string;
  ts?: string;
  payload?: Record<string, unknown>;
}

interface GoalState {
  sessionId: string | null;
  goal: Goal | null;
  history: GoalHistoryEntry[];
  loading: boolean;
  error: string | null;
  refreshGoal: (sessionId?: string | null) => Promise<void>;
  /** refreshGoal 的语义化别名，供会话切换接线。 */
  loadGoal: (sessionId: string | null) => Promise<void>;
  setGoal: (objective: string, tokenBudget?: number) => Promise<Goal | null>;
  pauseGoal: () => Promise<Goal | null>;
  resumeGoal: () => Promise<Goal | null>;
  clearGoal: () => Promise<boolean>;
  applyGoalEvent: (event: GoalEventLike, replaying?: boolean) => void;
  reset: () => void;
}

let loadToken = 0;

function asNumber(value: unknown, fallback = 0): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function asStatus(value: unknown, fallback: GoalStatus = 'active'): GoalStatus {
  return value === 'paused' || value === 'completed' || value === 'blocked' || value === 'active'
    ? value
    : fallback;
}

function asTimestamp(value: unknown, fallback?: string): string | undefined {
  if (typeof value === 'string') return value;
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback;
  // app-server 当前使用 Unix 整数；兼容秒与毫秒两种历史 wire。
  const milliseconds = Math.abs(value) < 10_000_000_000 ? value * 1000 : value;
  const date = new Date(milliseconds);
  return Number.isNaN(date.getTime()) ? fallback : date.toISOString();
}

function normalizeGoal(
  raw: GoalFace | Record<string, unknown>,
  sessionId: string,
  engine: unknown,
  previous?: Goal | null,
): Goal {
  const rec = raw as GoalFace & Record<string, unknown>;
  return {
    sessionId: typeof rec.sessionId === 'string' ? rec.sessionId : sessionId,
    objective: typeof rec.objective === 'string' ? rec.objective : (previous?.objective ?? ''),
    status: asStatus(rec.status, previous?.status),
    engine: (rec.engine ?? engine) === 'codex' ? 'codex' : 'local',
    tokenBudget: asNumber(rec.tokenBudget, previous?.tokenBudget),
    tokensUsed: asNumber(rec.tokensUsed, previous?.tokensUsed),
    timeUsedSeconds: asNumber(rec.timeUsedSeconds, previous?.timeUsedSeconds),
    turns: asNumber(rec.turns, previous?.turns),
    note: typeof rec.note === 'string' || rec.note === null ? rec.note : previous?.note,
    budgetExhausted:
      typeof rec.budgetExhausted === 'boolean'
        ? rec.budgetExhausted
        : previous?.budgetExhausted,
    createdAt: asTimestamp(rec.createdAt, previous?.createdAt),
    updatedAt: asTimestamp(rec.updatedAt, previous?.updatedAt),
  };
}

function pushHistory(history: GoalHistoryEntry[], goal: Goal, ts?: string): GoalHistoryEntry[] {
  const entry: GoalHistoryEntry = {
    status: goal.status,
    ts: ts ?? goal.updatedAt ?? new Date().toISOString(),
    note: goal.note,
    tokensUsed: goal.tokensUsed,
    timeUsedSeconds: goal.timeUsedSeconds,
  };
  const last = history[history.length - 1];
  if (
    last?.status === entry.status &&
    last.ts === entry.ts &&
    last.note === entry.note &&
    last.tokensUsed === entry.tokensUsed
  ) {
    return history;
  }
  return [...history, entry].slice(-100);
}

function activeSessionId(): string | null {
  return useSessionStore.getState().activeSessionId;
}

function report(err: unknown, prefix: string): string {
  const message = err instanceof Error ? err.message : String(err);
  useToastStore.getState().push('error', `${prefix}:${message}`);
  return message;
}

export const useGoalStore = create<GoalState>((set, get) => {
  const commit = (goal: Goal, open: boolean, ts?: string) => {
    set((state) => ({
      sessionId: goal.sessionId,
      goal,
      history: state.sessionId === goal.sessionId ? pushHistory(state.history, goal, ts) : pushHistory([], goal, ts),
      loading: false,
      error: null,
    }));
    if (open) useWorkbenchStore.getState().openTab('goal');
  };

  return {
    sessionId: null,
    goal: null,
    history: [],
    loading: false,
    error: null,

    refreshGoal: async (requested) => {
      const sessionId = requested === undefined ? activeSessionId() : requested;
      const token = (loadToken += 1);
      if (!sessionId) {
        set({ sessionId: null, goal: null, history: [], loading: false, error: null });
        return;
      }
      set({ sessionId, goal: null, history: [], loading: true, error: null });
      try {
        const response = await getGoal(sessionId);
        if (token !== loadToken || get().sessionId !== sessionId) return;
        if (!response.goal) {
          set({ goal: null, loading: false, error: null });
          return;
        }
        commit(normalizeGoal(response.goal, sessionId, response.engine), false);
      } catch (err) {
        if (token !== loadToken || get().sessionId !== sessionId) return;
        set({ loading: false, error: report(err, '目标加载失败') });
      }
    },

    loadGoal: async (sessionId) => get().refreshGoal(sessionId),

    setGoal: async (objective, tokenBudget) => {
      const sessionId = activeSessionId();
      const trimmed = objective.trim();
      if (!sessionId || trimmed === '') return null;
      const token = (loadToken += 1); // 取消会话切换时仍在途的旧 GET。
      try {
        const response = await putGoal(sessionId, {
          objective: trimmed,
          ...(tokenBudget !== undefined ? { tokenBudget } : {}),
        });
        const goal = normalizeGoal(response.goal, sessionId, response.goal.engine);
        if (token !== loadToken || activeSessionId() !== sessionId) return goal;
        commit(goal, true);
        return goal;
      } catch (err) {
        if (token === loadToken && activeSessionId() === sessionId) {
          set({ error: report(err, '设置目标失败') });
        }
        return null;
      }
    },

    pauseGoal: async () => {
      const sessionId = activeSessionId();
      if (!sessionId) return null;
      const token = (loadToken += 1);
      try {
        const response = await postGoalPause(sessionId);
        const goal = normalizeGoal(response.goal, sessionId, response.goal.engine, get().goal);
        if (token !== loadToken || activeSessionId() !== sessionId) return goal;
        commit(goal, false);
        return goal;
      } catch (err) {
        if (token === loadToken && activeSessionId() === sessionId) {
          set({ error: report(err, '暂停目标失败') });
        }
        return null;
      }
    },

    resumeGoal: async () => {
      const sessionId = activeSessionId();
      if (!sessionId) return null;
      const token = (loadToken += 1);
      try {
        const response = await postGoalResume(sessionId);
        const goal = normalizeGoal(response.goal, sessionId, response.goal.engine, get().goal);
        if (token !== loadToken || activeSessionId() !== sessionId) return goal;
        commit(goal, false);
        return goal;
      } catch (err) {
        if (token === loadToken && activeSessionId() === sessionId) {
          set({ error: report(err, '恢复目标失败') });
        }
        return null;
      }
    },

    clearGoal: async () => {
      const sessionId = activeSessionId();
      if (!sessionId) return false;
      const token = (loadToken += 1);
      try {
        await deleteGoal(sessionId);
        if (token !== loadToken || activeSessionId() !== sessionId) return true;
        set((state) => ({
          goal: null,
          history: [
            ...state.history,
            { status: 'cleared', ts: new Date().toISOString() } satisfies GoalHistoryEntry,
          ].slice(-100),
          error: null,
        }));
        return true;
      } catch (err) {
        if (token === loadToken && activeSessionId() === sessionId) {
          set({ error: report(err, '清除目标失败') });
        }
        return false;
      }
    },

    applyGoalEvent: (event, replaying = false) => {
      if (event.type !== 'goal.updated' && event.type !== 'goal.cleared') return;
      const sessionId = event.sessionId ?? get().sessionId ?? activeSessionId();
      if (!sessionId || activeSessionId() !== sessionId) return;
      loadToken += 1; // 当前会话实时事件比任何在途 GET/写响应更新，阻止陈旧结果回盖。
      if (event.type === 'goal.cleared') {
        const history = get().sessionId === sessionId ? get().history : [];
        set({
          sessionId,
          goal: null,
          history: [
            ...history,
            {
              status: 'cleared',
              ts: event.ts ?? new Date().toISOString(),
            } satisfies GoalHistoryEntry,
          ].slice(-100),
          loading: false,
          error: null,
        });
        return;
      }
      const raw = event.payload?.goal;
      if (!raw || typeof raw !== 'object') return;
      const firstForSession = get().sessionId !== sessionId || get().goal === null;
      const goal = normalizeGoal(
        raw as Record<string, unknown>,
        sessionId,
        event.payload?.engine,
        get().sessionId === sessionId ? get().goal : null,
      );
      commit(goal, firstForSession && !replaying, event.ts);
    },

    reset: () => {
      loadToken += 1;
      set({ sessionId: null, goal: null, history: [], loading: false, error: null });
    },
  };
});
