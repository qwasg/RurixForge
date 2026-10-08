import { useEffect } from 'react';

/**
 * 引用计数轮询器:首个订阅组件挂载时启动(立即跑一次),最后一个卸载时停止;
 * 页面隐藏时暂停计时,回到前台立即补一次。多个组件共用同一份数据时只起一个定时器。
 * onStart/onStop 挂接「事件触发的额外刷新」(如切工作区、run 结束),只在有订阅者时生效。
 */
export interface Poller {
  acquire: () => () => void;
}

export function createPoller(
  tick: () => void | Promise<void>,
  intervalMs: number,
  hooks: { onStart?: () => () => void } = {},
): Poller {
  let subscribers = 0;
  let timer: ReturnType<typeof setInterval> | null = null;
  let detach: (() => void) | null = null;

  const run = () => {
    void tick();
  };
  const startTimer = () => {
    if (timer === null) timer = setInterval(run, intervalMs);
  };
  const stopTimer = () => {
    if (timer !== null) {
      clearInterval(timer);
      timer = null;
    }
  };
  const hidden = () => typeof document !== 'undefined' && document.visibilityState === 'hidden';
  const onVisibility = () => {
    if (hidden()) {
      stopTimer();
    } else {
      run();
      startTimer();
    }
  };

  return {
    acquire() {
      subscribers += 1;
      if (subscribers === 1) {
        run();
        if (!hidden()) startTimer();
        document.addEventListener('visibilitychange', onVisibility);
        detach = hooks.onStart?.() ?? null;
      }
      let released = false;
      return () => {
        if (released) return;
        released = true;
        subscribers -= 1;
        if (subscribers === 0) {
          stopTimer();
          document.removeEventListener('visibilitychange', onVisibility);
          detach?.();
          detach = null;
        }
      };
    },
  };
}

/** 组件挂载期间订阅某个轮询器。 */
export function usePoller(poller: Poller): void {
  useEffect(() => poller.acquire(), [poller]);
}
