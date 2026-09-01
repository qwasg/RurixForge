import { createContext, type Context, type PluginFn, type PluginHandle } from './ctx.js';
import { configPlugin, type ConfigPatch } from './plugins/config.js';
import { loggerPlugin } from './plugins/logger.js';
import { eventlogPlugin } from './plugins/eventlog.js';
import { sessionsPlugin } from './plugins/sessions.js';
import { forgeProxyPlugin } from './plugins/forgeProxy.js';
import { httpPlugin } from './plugins/http.js';

/** bundle = 插件工厂数组 */
export type Bundle = PluginFn[];

export interface LoadedProfile {
  ctx: Context;
  unload(): void;
}

/**
 * profile/bundle 分层:profile = 有序 bundle 列表。
 * base 提供核心服务,web 叠加 HTTP 能力。
 */
export function loadProfile(name = 'web', patch: ConfigPatch = {}): LoadedProfile {
  const baseBundle: Bundle = [configPlugin(patch), loggerPlugin, eventlogPlugin, sessionsPlugin];
  // forgeProxy 须在 http 之前装载(http 初始化时读取该服务)
  const webBundle: Bundle = [forgeProxyPlugin(), httpPlugin];
  const profiles: Record<string, Bundle[]> = {
    base: [baseBundle],
    web: [baseBundle, webBundle],
  };
  const bundles = profiles[name];
  if (!bundles) throw new Error(`[forge-host] unknown profile: ${name}`);

  const ctx = createContext();
  const handles: PluginHandle[] = [];
  for (const bundle of bundles) {
    for (const plugin of bundle) handles.push(ctx.plugin(plugin));
  }
  return {
    ctx,
    unload() {
      for (let i = handles.length - 1; i >= 0; i--) handles[i].unload();
    },
  };
}
