import { create } from 'zustand';

/**
 * F7 wave.5 设置体系 store(参考 ui/settings.rs 语义适配):
 * - settingsPage:当前页(外观/Agent/模型/技能/关于),持久化 forge:settingsPage;
 * - submitCtrlEnter:Ctrl+Enter 发送(localStorage forge:submitCtrl,参考 moonlit:s:submitCtrl;
 *   Composer 消费——开启后 Ctrl+Enter 发送、Enter 换行);
 * - 开关语义:overlayStore.settings 驱动(互斥 closeAll 既有)。
 *
 * 差异留痕:参考九页(通用/外观/套餐与用量/Agent/自动补全/模型/规则·技能/工具与 MCP/记忆),
 * 本仓落地五页——套餐/auth(RD-F7-001)、自动补全/工具 MCP/记忆(RD-F7-002)无后端面,不造空页。
 */

export type SettingsPage = 'appearance' | 'agent' | 'models' | 'skills' | 'about';

export const SETTINGS_PAGES: Array<{ id: SettingsPage; label: string }> = [
  { id: 'appearance', label: '外观' },
  { id: 'agent', label: 'Agent' },
  { id: 'models', label: '模型' },
  { id: 'skills', label: '技能' },
  { id: 'about', label: '关于' },
];

const PAGE_KEY = 'forge:settingsPage';
const SUBMIT_CTRL_KEY = 'forge:submitCtrl';

function loadPage(): SettingsPage {
  try {
    const raw = globalThis.localStorage?.getItem(PAGE_KEY);
    if (SETTINGS_PAGES.some((p) => p.id === raw)) return raw as SettingsPage;
  } catch {
    // 读不进 → 默认
  }
  return 'appearance';
}

function loadSubmitCtrl(): boolean {
  try {
    return globalThis.localStorage?.getItem(SUBMIT_CTRL_KEY) === '1';
  } catch {
    return false;
  }
}

interface SettingsState {
  page: SettingsPage;
  submitCtrlEnter: boolean;
  setPage: (p: SettingsPage) => void;
  setSubmitCtrlEnter: (v: boolean) => void;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  page: loadPage(),
  submitCtrlEnter: loadSubmitCtrl(),

  setPage: (p) => {
    set({ page: p });
    try {
      globalThis.localStorage?.setItem(PAGE_KEY, p);
    } catch {
      // 写不进静默
    }
  },

  setSubmitCtrlEnter: (v) => {
    set({ submitCtrlEnter: v });
    try {
      globalThis.localStorage?.setItem(SUBMIT_CTRL_KEY, v ? '1' : '0');
    } catch {
      // 写不进静默
    }
  },
}));
