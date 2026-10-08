import { create } from 'zustand';

/**
 * F7 wave.5 Composer 外部预填 seam(Plan tab「开始 Build」消费):
 * prefill(text, mode) → Composer useEffect 订阅 token 变化应用 draft/mode 并聚焦。
 * 一次性消费:应用后 clear(防重复灌入)。
 */

interface ComposerPrefillState {
  draft: string | null;
  mode: string | null;
  /** 单调递增触发器(同文重填也生效)。 */
  token: number;
  prefill: (text: string, mode: string) => void;
  clear: () => void;
}

export const useComposerPrefillStore = create<ComposerPrefillState>((set) => ({
  draft: null,
  mode: null,
  token: 0,
  prefill: (text, mode) => set((st) => ({ draft: text, mode, token: st.token + 1 })),
  clear: () => set({ draft: null, mode: null }),
}));

const TEXT_DRAFT_KEY = 'forge:composer-text-drafts:v1';
function loadTextDrafts(): Record<string, string> {
  try { return JSON.parse(localStorage.getItem(TEXT_DRAFT_KEY) ?? '{}'); } catch { return {}; }
}
const useTextDraftStore = create<{ drafts: Record<string, string>; write: (key: string, value: string | ((old: string) => string)) => void }>((set) => ({
  drafts: loadTextDrafts(),
  write: (key, value) => set((state) => {
    const next = typeof value === 'function' ? value(state.drafts[key] ?? '') : value;
    const drafts = { ...state.drafts, [key]: next };
    try { localStorage.setItem(TEXT_DRAFT_KEY, JSON.stringify(drafts)); } catch { /* preserve memory draft */ }
    return { drafts };
  }),
}));
export function useComposerTextDraft(key: string): [string, (value: string | ((old: string) => string)) => void] {
  const text = useTextDraftStore((s) => s.drafts[key] ?? '');
  return [text, (value) => useTextDraftStore.getState().write(key, value)];
}
export function clearComposerDrafts(): void {
  useTextDraftStore.setState({ drafts: {} });
  try { localStorage.removeItem(TEXT_DRAFT_KEY); } catch { /* storage may be unavailable, as with draft reads/writes */ }
}
