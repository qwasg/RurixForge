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
