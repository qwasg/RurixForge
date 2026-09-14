import { act, cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import CommandWallAnimation from '@/components/game/CommandWallAnimation';
import { INITIAL_V4 } from '@/lib/sentinelsV4';
import { INITIAL_V5_ANIMATION, V5_EVENT, type V5AnimationState } from '@/lib/sentinelsV5';
import type { VideoAtlas } from '@/lib/sentinelsAnimationAssets';

const drawing = vi.hoisted(() => ({ draw: vi.fn(), wall: null as VideoAtlas | null, effect: null as VideoAtlas | null }));
vi.mock('@/lib/sentinelsAnimationAssets', async importOriginal => {
  const actual = await importOriginal<typeof import('@/lib/sentinelsAnimationAssets')>();
  return { ...actual, loadVideoAtlas: vi.fn(async (relative: string) => relative.startsWith('buildings/') ? drawing.wall! : drawing.effect!), drawVideoFrame: drawing.draw };
});
let now = 0, raf: FrameRequestCallback | null = null;
const state = (wall = true) => ({ ...INITIAL_V4, wallCells: [wall], shieldCells: [false] });
function animation(time: number, type: number = V5_EVENT.wallLand, active = true): V5AnimationState {
  return { ...INITIAL_V5_ANIMATION, time, eventSeq: 1, events: [{ seq: 1, type, kind: 10, x: -15.5, y: 9.5,
    age: time, duration: type === V5_EVENT.wallDestroy ? 4.5 : 2, subject: 0, owner: 0, magnitude: 20,
    fxKind: type === V5_EVENT.wallHit ? 3 : 0, active }] };
}
const pulse = (milliseconds: number) => { now = milliseconds; act(() => { raf?.(now); }); };
const lastWallFrame = () => drawing.draw.mock.calls.filter(call => call[1] === drawing.wall).at(-1)?.[2];
beforeEach(() => {
  now = 0; raf = null; drawing.draw.mockReset();
  drawing.wall = { image: document.createElement('img'), width: 256, height: 256, boxes: Array.from({ length: 128 }, () => [0, 0, 1, 1]), normalizationSpan: .640625,
    clips: { land: { start: 0, endExclusive: 32, fps: 16, loop: false }, work: { start: 32, endExclusive: 80, fps: 16, loop: true }, destroy: { start: 80, endExclusive: 128, fps: 24, loop: false } } };
  drawing.effect = { ...drawing.wall, boxes: drawing.wall.boxes.slice(0, 48), clips: { oneshot: { start: 0, endExclusive: 48, fps: 24, loop: false } } };
  vi.spyOn(performance, 'now').mockImplementation(() => now);
  vi.stubGlobal('requestAnimationFrame', vi.fn((callback: FrameRequestCallback) => { raf = callback; return 1; }));
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({ clearRect: vi.fn(), save: vi.fn(), restore: vi.fn() } as unknown as CanvasRenderingContext2D);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe('native clock wall video playback', () => {
  it('never exceeds the published clock and freezes across a long pause', async () => {
    const props = { state: state(), animation: animation(0), paused: false, connected: true, onReady: vi.fn(), onError: vi.fn() };
    const view = render(<CommandWallAnimation {...props}/>); await act(async () => {});
    pulse(10_000); expect(lastWallFrame()).toBe(0);
    view.rerender(<CommandWallAnimation {...props} animation={animation(1)}/>);
    pulse(10_500); expect(lastWallFrame()).toBe(8);
    view.rerender(<CommandWallAnimation {...props} animation={animation(1)} paused/>);
    pulse(100_000); expect(lastWallFrame()).toBe(8);
    view.rerender(<CommandWallAnimation {...props} animation={animation(1)}/>);
    pulse(102_000); expect(lastWallFrame()).toBe(16);
    pulse(200_000); expect(lastWallFrame()).toBe(16);
  });

  it('resynchronizes to fresh native time after disconnect without replaying an old birth', async () => {
    const props = { state: state(), animation: animation(0), paused: false, connected: true, onReady: vi.fn(), onError: vi.fn() };
    const view = render(<CommandWallAnimation {...props}/>); await act(async () => {});
    view.rerender(<CommandWallAnimation {...props} animation={animation(1)}/>); pulse(500); expect(lastWallFrame()).toBe(8);
    view.rerender(<CommandWallAnimation {...props} animation={animation(1)} connected={false}/>);
    pulse(60_000); expect(lastWallFrame()).toBe(8);
    view.rerender(<CommandWallAnimation {...props} animation={animation(20, V5_EVENT.wallLand, false)}/>);
    pulse(60_000); expect(lastWallFrame()).toBe(32);
    view.rerender(<CommandWallAnimation {...props} animation={animation(20, V5_EVENT.wallLand, true)}/>);
    pulse(60_100); expect(lastWallFrame()).toBe(32);
  });

  it('holds the final destruction frame only within its finite rubble tail', async () => {
    const props = { state: state(false), animation: animation(0, V5_EVENT.wallDestroy), paused: false, connected: true, onReady: vi.fn(), onError: vi.fn() };
    const view = render(<CommandWallAnimation {...props}/>); await act(async () => {});
    view.rerender(<CommandWallAnimation {...props} animation={animation(4.4, V5_EVENT.wallDestroy)}/>);
    pulse(4_400); expect(lastWallFrame()).toBe(127); expect(drawing.draw.mock.calls.at(-1)?.[7]).toBeCloseTo(.04);
    view.rerender(<CommandWallAnimation {...props} animation={animation(4.5, V5_EVENT.wallDestroy, false)}/>);
    drawing.draw.mockClear(); pulse(4_600); expect(drawing.draw).not.toHaveBeenCalled();
    pulse(100_000); expect(drawing.draw).not.toHaveBeenCalled();
  });

  it('finishes a hit clip after its native ring entry expires and never loops it', async () => {
    const props = { state: state(false), animation: animation(0, V5_EVENT.wallHit), paused: false, connected: true, onReady: vi.fn(), onError: vi.fn() };
    const view = render(<CommandWallAnimation {...props}/>); await act(async () => {});
    view.rerender(<CommandWallAnimation {...props} animation={animation(2, V5_EVENT.wallHit, false)}/>);
    pulse(1_900); expect(drawing.draw.mock.calls.at(-1)?.[2]).toBe(45);
    drawing.draw.mockClear(); pulse(2_000); expect(drawing.draw).not.toHaveBeenCalled();
    pulse(50_000); expect(drawing.draw).not.toHaveBeenCalled();
  });
});
