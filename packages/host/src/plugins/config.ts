import path from 'node:path';
import type { PluginFn } from '../ctx.js';

export interface HostConfig {
  host: string;
  port: number;
  dataDir: string;
  version: string;
}

export interface ConfigPatch {
  host?: string;
  port?: number;
  dataDir?: string;
}

/** 配置插件:patch 覆盖默认值,dataDir 默认 <cwd>/data */
export function configPlugin(patch: ConfigPatch = {}): PluginFn {
  return (ctx) => {
    ctx.provide('config', {
      host: patch.host ?? '127.0.0.1',
      port: patch.port ?? 3080,
      dataDir: patch.dataDir ?? path.resolve(process.cwd(), 'data'),
      version: '0.1.0',
    } satisfies HostConfig);
  };
}
