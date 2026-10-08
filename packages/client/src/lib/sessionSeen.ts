import { create } from 'zustand';
import type { ForgeSession } from './sessionStore';

/**
 * 会话「已读」记录(本机 localStorage,forge:sessionSeen):id → 最后查看时刻(ISO)。
 * 侧栏据此给后台会话画未读点——会话在你没看着的时候跑出了新结果(updatedAt 前进)。
 * 首次见到的会话以其当时的 updatedAt 为基线,所以老会话不会一上来全是未读。
 */

const KEY = 'forge:sessionSeen';

function load(): Record<string, string> {
  try {
    const raw = globalThis.localStorage?.getItem(KEY);
    const v: unknown = raw ? JSON.parse(raw) : {};
    return v && typeof v === 'object' ? (v as Record<string, string>) : {};
  } catch {
    return {};
  }
}

function save(seen: Record<string, string>): void {
  try {
    globalThis.localStorage?.setItem(KEY, JSON.stringify(seen));
  } catch {
    // 写不进静默
  }
}

function ts(iso: string | undefined): number {
  const n = iso ? Date.parse(iso) : Number.NaN;
  return Number.isNaN(n) ? 0 : n;
}

interface SeenState {
  seen: Record<string, string>;
  /** 为尚无记录的会话建立基线(= 当时的 updatedAt),并清掉已不存在的会话。 */
  observe: (sessions: ForgeSession[]) => void;
  /** 标记已读到「现在」与该会话 updatedAt 中较晚者。 */
  markSeen: (session: ForgeSession) => void;
}

export const useSessionSeen = create<SeenState>((set, get) => ({
  seen: load(),

  observe: (sessions) => {
    const cur = get().seen;
    const live = new Set(sessions.map((s) => s.id));
    let changed = false;
    const next: Record<string, string> = {};
    for (const [id, at] of Object.entries(cur)) {
      if (live.has(id)) next[id] = at;
      else changed = true;
    }
    for (const s of sessions) {
      if (next[s.id] === undefined) {
        next[s.id] = s.updatedAt || new Date().toISOString();
        changed = true;
      }
    }
    if (!changed) return;
    save(next);
    set({ seen: next });
  },

  markSeen: (session) => {
    const now = new Date().toISOString();
    const at = ts(session.updatedAt) > Date.parse(now) ? session.updatedAt : now;
    if (get().seen[session.id] === at) return;
    const next = { ...get().seen, [session.id]: at };
    save(next);
    set({ seen: next });
  },
}));

/** 未读:不是当前会话、没在跑,且 updatedAt 晚于最后查看时刻。 */
export function isUnread(session: ForgeSession, seenAt: string | undefined, activeSessionId: string | null): boolean {
  if (session.id === activeSessionId || session.activeRunId != null || seenAt === undefined) return false;
  return ts(session.updatedAt) > ts(seenAt);
}
