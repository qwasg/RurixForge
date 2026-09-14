import { create } from 'zustand';

/**
 * F7 wave.3 toast 体系:右下堆叠(bottom 46/right 20/gap 8),3px 类型色竖条 +
 * 6px 状态点 + 12px 标题;2.8s 自动消失。
 * 类型色:success=sage / error=danger / warning=warn / info=accent(参考 overlays.rs)。
 */

export type ToastKind = 'success' | 'error' | 'warning' | 'info';

export interface ToastItem {
  id: number;
  kind: ToastKind;
  title: string;
}

export const TOAST_TTL_MS = 2800;

interface ToastState {
  items: ToastItem[];
  push: (kind: ToastKind, title: string) => number;
  dismiss: (id: number) => void;
  clear: () => void;
}

let nextId = 1;

export const useToastStore = create<ToastState>((set) => ({
  items: [],
  push: (kind, title) => {
    const id = nextId++;
    set((st) => ({ items: [...st.items, { id, kind, title }] }));
    // 2.8s 自动消失(测试用 fake timers 可断言)
    setTimeout(() => {
      useToastStore.getState().dismiss(id);
    }, TOAST_TTL_MS);
    return id;
  },
  dismiss: (id) => set((st) => ({ items: st.items.filter((t) => t.id !== id) })),
  clear: () => set({ items: [] }),
}));
