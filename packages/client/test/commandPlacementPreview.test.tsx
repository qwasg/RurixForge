import { act, cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from 'vitest';
import CommandPlacementPreview from '@/components/game/CommandPlacementPreview';
import type { VideoAtlas } from '@/lib/sentinelsAnimationAssets';

const loading = vi.hoisted(() => ({ atlas: vi.fn() }));
vi.mock('@/lib/sentinelsAnimationAssets', () => ({ loadVideoAtlas: loading.atlas }));
let source: VideoAtlas, drawImage: ReturnType<typeof vi.fn>, toDataURL: MockInstance<HTMLCanvasElement['toDataURL']>, canvases: HTMLCanvasElement[];
const cropped = 'data:image/png;base64,Y3JvcHBlZC13b3JrLWZyYW1l';
let fixtureId = 0;
let props = { asset: 'data-center', x: 12, y: 8, footprint: 2, fallback: '/legacy/data-center.png', fallbackSize: 2.56 };
const preview = (values = props) => <svg className="command-map-overlay" viewBox="0 0 35.5555556 20"><CommandPlacementPreview {...values}/></svg>;
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }

beforeEach(() => {
  props = { ...props, asset: 'data-center-fixture-' + ++fixtureId };
  const image = document.createElement('img'); image.src = '/games/code-sentinels/animation-v5/buildings/data-center.png';
  source = { image, width: 2582, height: 3356, normalizationSpan: .640625,
    boxes: Array.from({ length: 128 }, (_, index) => [2 + index % 10 * 258, 2 + Math.floor(index / 10) * 258, 256, 256]),
    clips: { land: { start: 0, endExclusive: 32, fps: 16, loop: false }, work: { start: 32, endExclusive: 80, fps: 16, loop: true }, destroy: { start: 80, endExclusive: 128, fps: 24, loop: false } } };
  loading.atlas.mockReset().mockResolvedValue(source); drawImage = vi.fn(); canvases = [];
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    canvases.push(this); return { drawImage } as unknown as CanvasRenderingContext2D;
  });
  toDataURL = vi.spyOn(HTMLCanvasElement.prototype, 'toDataURL').mockReturnValue(cropped);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe('single-frame placement preview', () => {
  it('crops the authentic work-start frame before inserting any image into the world SVG', async () => {
    const view = render(preview()); await act(async () => {});
    expect(loading.atlas).toHaveBeenCalledExactlyOnceWith('buildings/' + props.asset);
    expect(drawImage).toHaveBeenCalledExactlyOnceWith(source.image, 518, 776, 256, 256, 0, 0, 256, 256);
    expect(canvases[0].width).toBe(256); expect(canvases[0].height).toBe(256);
    expect(toDataURL).toHaveBeenCalledExactlyOnceWith('image/png');
    const image = view.container.querySelector('image')!;
    expect(image).toHaveAttribute('href', cropped);
    expect(view.container.querySelectorAll('svg')).toHaveLength(1);
    expect(view.container.querySelector('svg svg')).toBeNull();
    expect(view.container.innerHTML).not.toContain(source.image.src);
    // Broad world SVG sizing/overflow rules can no longer expose atlas pixels:
    // the only image supplied to the SVG renderer contains one 256px frame.
    expect(image.tagName.toLowerCase()).toBe('image');
  });

  it.each([1, 2, 3])('preserves the native center and normalized %i-cell footprint', async footprint => {
    const view = render(preview({ ...props, footprint })); await act(async () => {});
    const image = view.container.querySelector('image')!, size = footprint / source.normalizationSpan;
    expect(Number(image.getAttribute('width'))).toBeCloseTo(size);
    expect(Number(image.getAttribute('height'))).toBeCloseTo(size);
    expect(Number(image.getAttribute('x')) + size / 2).toBeCloseTo(props.x);
    expect(Number(image.getAttribute('y')) + size / 2).toBeCloseTo(props.y);
  });

  it('does not replace a newly selected preview with an older asynchronous atlas load', async () => {
    const first = deferred<VideoAtlas>(), second = deferred<VideoAtlas>();
    const currentFrame = cropped + '-current', oldFrame = cropped + '-old';
    toDataURL.mockReturnValueOnce(currentFrame).mockReturnValueOnce(oldFrame);
    loading.atlas.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const view = render(preview()); view.rerender(preview({ ...props, asset: props.asset + '-research-lab' }));
    await act(async () => second.resolve(source));
    expect(view.container.querySelector('image')).toHaveAttribute('href', currentFrame);
    await act(async () => first.resolve(source));
    expect(drawImage).toHaveBeenCalledTimes(2); expect(toDataURL).toHaveBeenCalledTimes(2);
    expect(view.container.querySelector('image')).toHaveAttribute('href', currentFrame);
  });

  it('reuses the same asset crop after the hover preview unmounts and returns', async () => {
    const first = render(preview()); await act(async () => {}); first.unmount();
    const second = render(preview()); await act(async () => {});
    expect(loading.atlas).toHaveBeenCalledTimes(1); expect(drawImage).toHaveBeenCalledTimes(1);
    expect(toDataURL).toHaveBeenCalledTimes(1); expect(second.container.querySelector('image')).toHaveAttribute('href', cropped);
  });

  it('keeps a bounded fallback image if the browser cannot create a frame crop', async () => {
    vi.mocked(HTMLCanvasElement.prototype.getContext).mockReturnValue(null);
    const view = render(preview()); await act(async () => {});
    expect(view.container.querySelector('image')).toHaveAttribute('href', props.fallback);
    expect(view.container.querySelector('image')).toHaveAttribute('width', String(props.fallbackSize));
    expect(view.container.querySelectorAll('svg')).toHaveLength(1); expect(toDataURL).not.toHaveBeenCalled();
  });
});
