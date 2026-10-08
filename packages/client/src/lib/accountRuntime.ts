import { useEffect } from 'react';
import { useAccountStore } from './accountStore';
import { useChatStore } from './chatStore';
import { pullCloudSettings, startSettingsSync } from './settingsSync';

/**
 * 云账户运行时(应用壳挂载一次):启动即拉状态、每 60s 轮询、每轮 agent 结束后刷新余额;
 * 登录态翻转时拉云端设置并重拉模型目录(云端条目的 availability 随登录态变)。
 */

export const ACCOUNT_POLL_MS = 60_000;
/** agent.completed/failed 后刷新余额的合并窗口(同一轮的尾部事件只触发一次)。 */
export const ACCOUNT_EVENT_REFRESH_MS = 1_500;
/** 快照回放里的历史终态事件不触发刷新。 */
const RECENT_EVENT_MS = 5 * 60_000;

function isRecent(ts: string | undefined): boolean {
  const t = ts ? Date.parse(ts) : Number.NaN;
  return Number.isFinite(t) ? Date.now() - t < RECENT_EVENT_MS : true;
}

export function startAccountRuntime(): () => void {
  const account = useAccountStore;
  void account.getState().refreshStatus();
  const poll = setInterval(() => void account.getState().refreshStatus(), ACCOUNT_POLL_MS);

  let eventTimer: ReturnType<typeof setTimeout> | null = null;
  const offChat = useChatStore.subscribe((st, prev) => {
    if (st.eventsRing === prev.eventsRing) return;
    const last = st.eventsRing[st.eventsRing.length - 1];
    if (!last || (last.type !== 'agent.completed' && last.type !== 'agent.failed')) return;
    if (!isRecent(last.ts)) return;
    if (eventTimer) clearTimeout(eventTimer);
    eventTimer = setTimeout(() => {
      eventTimer = null;
      void account.getState().refreshStatus();
    }, ACCOUNT_EVENT_REFRESH_MS);
  });

  const offAccount = account.subscribe((st, prev) => {
    const now = st.status?.loggedIn === true;
    const was = prev.status?.loggedIn === true;
    if (now === was) return;
    if (now) void pullCloudSettings();
    void useChatStore.getState().ensureModels(true);
  });
  if (account.getState().status?.loggedIn) void pullCloudSettings();

  const stopSync = startSettingsSync();
  return () => {
    clearInterval(poll);
    if (eventTimer) clearTimeout(eventTimer);
    offChat();
    offAccount();
    stopSync();
  };
}

export function useAccountRuntime(): void {
  useEffect(() => startAccountRuntime(), []);
}
