import { describe, it, expect, vi } from 'vitest';
import { createContext } from '../src/ctx.js';

describe('ctx 插件内核', () => {
  it('provide/get 存取服务,未注册抛错', () => {
    const ctx = createContext();
    ctx.provide('foo', { v: 42 });
    expect(ctx.get<{ v: number }>('foo').v).toBe(42);
    expect(() => ctx.get('missing')).toThrow(/not provided/);
  });

  it('on/emit 事件收发,off 移除监听', () => {
    const ctx = createContext();
    const fn = vi.fn();
    ctx.on('tick', fn);
    ctx.emit('tick', { n: 1 });
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenCalledWith({ n: 1 });
    ctx.off('tick', fn);
    ctx.emit('tick', { n: 2 });
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('plugin unload 后服务消失且监听器不再触发', () => {
    const ctx = createContext();
    const fn = vi.fn();
    const handle = ctx.plugin((c) => {
      c.provide('svc', { name: 'temp' });
      c.on('ping', fn);
    });
    expect(ctx.get<{ name: string }>('svc').name).toBe('temp');
    ctx.emit('ping');
    expect(fn).toHaveBeenCalledTimes(1);

    handle.unload();
    expect(() => ctx.get('svc')).toThrow(/not provided/);
    ctx.emit('ping');
    expect(fn).toHaveBeenCalledTimes(1);
    // 幂等:重复 unload 不报错
    handle.unload();
  });

  it('插件返回的 dispose 在 unload 时调用', () => {
    const ctx = createContext();
    const dispose = vi.fn();
    const handle = ctx.plugin(() => dispose);
    handle.unload();
    expect(dispose).toHaveBeenCalledTimes(1);
  });
});
