import type { PluginFn } from '../ctx.js';

export interface Logger {
  info(msg: string, ...args: unknown[]): void;
  warn(msg: string, ...args: unknown[]): void;
  error(msg: string, ...args: unknown[]): void;
}

/** 日志插件:简单包装 console,统一前缀 */
export const loggerPlugin: PluginFn = (ctx) => {
  const prefix = '[forge-host]';
  ctx.provide('logger', {
    info: (msg, ...args) => console.log(prefix, msg, ...args),
    warn: (msg, ...args) => console.warn(prefix, msg, ...args),
    error: (msg, ...args) => console.error(prefix, msg, ...args),
  } satisfies Logger);
};
