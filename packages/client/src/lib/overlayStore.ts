import { create } from 'zustand';

/**
 * F7 wave.3 浮层体系:各 open 布尔 + closeAll()(全局 Esc 绑定,对齐参考 close_overlays)。
 * 互斥:打开任一浮层时关闭其他(命令面板/设置/关于/快捷键 同一时刻只一个)。
 */

export type OverlayKey = 'palette' | 'settings' | 'about' | 'shortcuts';

interface OverlayState {
  palette: boolean;
  settings: boolean;
  about: boolean;
  shortcuts: boolean;
  open: (key: OverlayKey) => void;
  close: (key: OverlayKey) => void;
  closeAll: () => void;
}

export const useOverlayStore = create<OverlayState>((set) => ({
  palette: false,
  settings: false,
  about: false,
  shortcuts: false,
  open: (key) =>
    set({ palette: false, settings: false, about: false, shortcuts: false, [key]: true }),
  close: (key) => set({ [key]: false }),
  closeAll: () => set({ palette: false, settings: false, about: false, shortcuts: false }),
}));
