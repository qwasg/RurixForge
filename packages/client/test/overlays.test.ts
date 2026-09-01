import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useOverlayStore } from '@/lib/overlayStore';
import { TOAST_TTL_MS, useToastStore } from '@/lib/toastStore';

/** F7 wave.3:overlay 互斥 / Esc closeAll / toast 2.8s 自动消失(fake timers)。 */

const initialOverlay = useOverlayStore.getState();

beforeEach(() => {
  useOverlayStore.setState(initialOverlay, true);
  useToastStore.getState().clear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('overlayStore', () => {
  it('open 互斥:同时只一个浮层开', () => {
    useOverlayStore.getState().open('palette');
    expect(useOverlayStore.getState().palette).toBe(true);
    useOverlayStore.getState().open('settings');
    const s = useOverlayStore.getState();
    expect(s.settings).toBe(true);
    expect(s.palette).toBe(false);
    expect(s.about).toBe(false);
  });

  it('closeAll:Esc 全关', () => {
    useOverlayStore.getState().open('about');
    useOverlayStore.getState().closeAll();
    const s = useOverlayStore.getState();
    expect(s.about).toBe(false);
    expect(s.palette).toBe(false);
    expect(s.settings).toBe(false);
    expect(s.shortcuts).toBe(false);
  });

  it('close 单个关闭', () => {
    useOverlayStore.getState().open('shortcuts');
    useOverlayStore.getState().close('shortcuts');
    expect(useOverlayStore.getState().shortcuts).toBe(false);
  });
});

describe('toastStore', () => {
  it('push 入栈 + dismiss 移除', () => {
    const id = useToastStore.getState().push('success', '已保存');
    expect(useToastStore.getState().items).toHaveLength(1);
    expect(useToastStore.getState().items[0]).toMatchObject({ kind: 'success', title: '已保存' });
    useToastStore.getState().dismiss(id);
    expect(useToastStore.getState().items).toHaveLength(0);
  });

  it('2.8s 自动消失(fake timers)', () => {
    vi.useFakeTimers();
    useToastStore.getState().push('info', '提示');
    expect(useToastStore.getState().items).toHaveLength(1);
    vi.advanceTimersByTime(TOAST_TTL_MS - 100);
    expect(useToastStore.getState().items).toHaveLength(1);
    vi.advanceTimersByTime(200);
    expect(useToastStore.getState().items).toHaveLength(0);
  });

  it('多条堆叠顺序', () => {
    vi.useFakeTimers();
    useToastStore.getState().push('error', '一');
    useToastStore.getState().push('warning', '二');
    const items = useToastStore.getState().items;
    expect(items.map((t) => t.title)).toEqual(['一', '二']);
    expect(items.map((t) => t.kind)).toEqual(['error', 'warning']);
  });
});
