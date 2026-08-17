import type { Context } from './ctx.js';
import { loadProfile } from './profile.js';
import type { ConfigPatch } from './plugins/config.js';
import type { HttpService } from './plugins/http.js';

export interface ForgeServer {
  ctx: Context;
  listen(port?: number): Promise<number>;
  close(): Promise<void>;
}

/** 组装 web profile,供入口与测试复用(port 传 0 可随机) */
export function buildServer(patch: ConfigPatch = {}): ForgeServer {
  const { ctx, unload } = loadProfile('web', patch);
  const http = ctx.get<HttpService>('http');
  return {
    ctx,
    listen: (port) => http.start(port),
    async close() {
      await http.close();
      unload();
    },
  };
}
