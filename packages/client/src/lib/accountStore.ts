import { create } from 'zustand';
import {
  errorMessage,
  getAccountStatus,
  getAuthConfig,
  postLogin,
  postLogout,
  postRegister,
  type AccountStatus,
  type AuthConfig,
  type RegisterPayload,
} from './accountApi';
import { useOverlayStore } from './overlayStore';
import { useSettingsStore } from './settingsStore';

/**
 * 云账户 store(D-041,15 §8.2):agentd 的 `/api/forge/account/status` 是唯一事实源。
 * 登录门条件 `!loggedIn && !byoConfigured && !devMock`;状态拿不到(null)时不弹门。
 * 「使用自带密钥」只在本次应用会话内放行(内存态,不落盘)。
 */

interface AccountState {
  status: AccountStatus | null;
  authConfig: AuthConfig | null;
  loading: boolean;
  error: string | null;
  /** 用户主动打开登录页(失败引导「登录」、设置页「登录」)。 */
  authOpen: boolean;
  /** 本次会话已选「自带密钥」绕过登录门。 */
  dismissed: boolean;

  refreshStatus: () => Promise<AccountStatus | null>;
  loadAuthConfig: () => Promise<AuthConfig | null>;
  /** 失败时抛出(表单就地显示 error.message)。 */
  login: (email: string, password: string) => Promise<AccountStatus | null>;
  register: (payload: RegisterPayload) => Promise<AccountStatus | null>;
  logout: () => Promise<void>;
  setStatus: (status: AccountStatus | null) => void;
  openAuth: () => void;
  closeAuth: () => void;
  dismissGateForSession: () => void;
}

/** 登录门是否显示(纯函数,供 AuthScreen 与测试共用)。 */
export function gateVisible(st: Pick<AccountState, 'status' | 'authOpen' | 'dismissed'>): boolean {
  if (st.authOpen) return true;
  const s = st.status;
  return s !== null && !s.loggedIn && !s.byoConfigured && !s.devMock && !st.dismissed;
}

/** 登录门是否为强制态(没有关闭钮,只能登录或走自带密钥)。 */
export function gateForced(st: Pick<AccountState, 'status' | 'dismissed'>): boolean {
  const s = st.status;
  return s !== null && !s.loggedIn && !s.byoConfigured && !s.devMock && !st.dismissed;
}

let inflight: Promise<AccountStatus | null> | null = null;

export const useAccountStore = create<AccountState>((set, get) => ({
  status: null,
  authConfig: null,
  loading: false,
  error: null,
  authOpen: false,
  dismissed: false,

  refreshStatus: () => {
    if (inflight) return inflight;
    set({ loading: true });
    const task = (async () => {
      try {
        const status = await getAccountStatus();
        if (status) set({ status, error: null });
        return status ?? get().status;
      } catch (err) {
        // agentd 不可达:保留上次状态(不因一次失败把已登录用户踢回登录门)
        set({ error: errorMessage(err) });
        return get().status;
      } finally {
        set({ loading: false });
      }
    })();
    inflight = task;
    void task.finally(() => {
      if (inflight === task) inflight = null;
    });
    return task;
  },

  loadAuthConfig: async () => {
    try {
      const authConfig = await getAuthConfig();
      set({ authConfig });
      return authConfig;
    } catch {
      // 云端不可达时注册表单按最宽松形态呈现,提交时由云端如实拒绝
      return get().authConfig;
    }
  },

  login: async (email, password) => {
    const status = await postLogin({ email, password });
    if (status) set({ status, error: null });
    if (status?.loggedIn) set({ authOpen: false });
    return status;
  },

  register: async (payload) => {
    const status = await postRegister(payload);
    if (status) set({ status, error: null });
    if (status?.loggedIn) set({ authOpen: false });
    return status;
  },

  logout: async () => {
    try {
      const status = await postLogout();
      if (status) set({ status, error: null });
      else await get().refreshStatus();
    } catch (err) {
      set({ error: errorMessage(err) });
      await get().refreshStatus();
    }
  },

  setStatus: (status) => set({ status }),

  openAuth: () => {
    // 登录页盖在最上层;设置等浮层让位,登录成功后回到原界面
    useOverlayStore.getState().closeAll();
    set({ authOpen: true });
  },

  closeAuth: () => set({ authOpen: false }),

  dismissGateForSession: () => {
    set({ dismissed: true, authOpen: false });
    useSettingsStore.getState().setPage('models');
    useOverlayStore.getState().open('settings');
  },
}));
