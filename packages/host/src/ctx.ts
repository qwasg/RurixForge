/**
 * Cordis 式插件内核:服务注册 + 类型化事件 + 可逆 effect。
 * 插件内的一切注册(provide/on)都会被跟踪,unload 时自动撤销。
 */

export type Listener = (payload: unknown) => void;
export type DisposeFn = () => void;
/** 插件函数:可返回 dispose,unload 时一并调用 */
export type PluginFn = (ctx: Context) => void | DisposeFn;

export interface PluginHandle {
  unload(): void;
}

export interface Context {
  provide(name: string, service: unknown): void;
  get<T>(name: string): T;
  on(event: string, fn: Listener): void;
  off(event: string, fn: Listener): void;
  emit(event: string, payload?: unknown): void;
  plugin(fn: PluginFn): PluginHandle;
}

export function createContext(): Context {
  const services = new Map<string, unknown>();
  const listeners = new Map<string, Set<Listener>>();

  /** record 存在时,所有注册动作同时记录撤销函数(effect 跟踪) */
  function makeCtx(record?: DisposeFn[]): Context {
    return {
      provide(name, service) {
        services.set(name, service);
        record?.push(() => {
          services.delete(name);
        });
      },
      get<T>(name: string): T {
        if (!services.has(name)) {
          throw new Error(`[forge-host] service not provided: ${name}`);
        }
        return services.get(name) as T;
      },
      on(event, fn) {
        let set = listeners.get(event);
        if (!set) {
          set = new Set();
          listeners.set(event, set);
        }
        set.add(fn);
        record?.push(() => {
          set.delete(fn);
        });
      },
      off(event, fn) {
        listeners.get(event)?.delete(fn);
      },
      emit(event, payload) {
        const set = listeners.get(event);
        if (!set) return;
        // 拷贝快照,避免回调内 off 影响遍历
        for (const fn of [...set]) fn(payload);
      },
      plugin(fn) {
        return applyPlugin(fn);
      },
    };
  }

  function applyPlugin(fn: PluginFn): PluginHandle {
    const disposers: DisposeFn[] = [];
    const scoped = makeCtx(disposers);
    const ownDispose = fn(scoped);
    if (typeof ownDispose === 'function') disposers.push(ownDispose);
    let unloaded = false;
    return {
      unload() {
        if (unloaded) return;
        unloaded = true;
        // 逆序撤销,模拟栈式清理
        for (let i = disposers.length - 1; i >= 0; i--) disposers[i]();
      },
    };
  }

  return makeCtx();
}
