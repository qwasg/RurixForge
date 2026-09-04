import { describe, expect, it } from 'vitest';
import { detectSpriteBoxes, isBackgroundPixel } from '@/lib/spriteAutoDetect';

/**
 * F-GAME-4 前端本地自动切帧纯函数:与服务端 autoslice_image 同规则
 * (背景 = alpha<5 或品红族 2*g < min(r,b);4 连通;minArea 过滤;行带排序)。
 * 用构造的 RGBA 缓冲断言,不依赖 canvas 解码面。
 */

/** 构造 w×h RGBA 缓冲,底色 fill,再把 boxes 区域涂成 color。 */
function makeImage(
  w: number,
  h: number,
  fill: [number, number, number, number],
  blobs: Array<{ x: number; y: number; w: number; h: number; color?: [number, number, number, number] }>,
): Uint8ClampedArray {
  const buf = new Uint8ClampedArray(w * h * 4);
  for (let i = 0; i < w * h; i++) buf.set(fill, i * 4);
  for (const b of blobs) {
    const c = b.color ?? [255, 255, 255, 255];
    for (let y = b.y; y < b.y + b.h; y++) {
      for (let x = b.x; x < b.x + b.w; x++) {
        buf.set(c, (y * w + x) * 4);
      }
    }
  }
  return buf;
}

const MAGENTA: [number, number, number, number] = [255, 0, 255, 255];
const TRANSPARENT: [number, number, number, number] = [0, 0, 0, 0];

describe('spriteAutoDetect(与服务端同规则)', () => {
  it('品红底:检出两个连通域,紧致 bbox + 行内从左到右', () => {
    // 两个 5×4 白块同一行(y=2),中间隔品红。
    const img = makeImage(24, 10, MAGENTA, [
      { x: 2, y: 2, w: 5, h: 4 },
      { x: 12, y: 3, w: 5, h: 4 },
    ]);
    const boxes = detectSpriteBoxes(24, 10, img);
    expect(boxes).toEqual([
      [2, 2, 5, 4],
      [12, 3, 5, 4],
    ]);
    // 色键规则字面核对:品红(2*0 < min(255,255))是背景,暗品红同族;绿色不是。
    expect(isBackgroundPixel(255, 0, 255, 255)).toBe(true);
    expect(isBackgroundPixel(120, 40, 130, 255)).toBe(true); // 2*40 < min(120,130)
    expect(isBackgroundPixel(0, 200, 0, 255)).toBe(false);
    expect(isBackgroundPixel(0, 0, 0, 4)).toBe(true); // alpha<5
  });

  it('透明底:多行连通域按行带分组(从上到下,行内从左到右)', () => {
    const img = makeImage(32, 20, TRANSPARENT, [
      // 第二行(y 大)先构造,断言输出仍按行序。
      { x: 18, y: 12, w: 6, h: 6, color: [10, 200, 30, 255] },
      { x: 2, y: 11, w: 6, h: 6, color: [10, 200, 30, 255] },
      { x: 9, y: 1, w: 8, h: 6, color: [200, 30, 30, 255] },
    ]);
    const boxes = detectSpriteBoxes(32, 20, img);
    expect(boxes).toEqual([
      [9, 1, 8, 6], // 第一行
      [2, 11, 6, 6], // 第二行,x 小者在前
      [18, 12, 6, 6],
    ]);
  });

  it('噪点过滤:像素数 < minArea 的连通域被丢弃;minArea 调低后保留', () => {
    // 3×3=9 像素小块(默认 minArea 16 → 滤掉)+ 5×5=25 大块(保留)。
    const img = makeImage(20, 10, TRANSPARENT, [
      { x: 1, y: 1, w: 3, h: 3 },
      { x: 10, y: 2, w: 5, h: 5 },
    ]);
    expect(detectSpriteBoxes(20, 10, img)).toEqual([[10, 2, 5, 5]]);
    expect(detectSpriteBoxes(20, 10, img, { minArea: 9 })).toEqual([
      [1, 1, 3, 3],
      [10, 2, 5, 5],
    ]);
  });

  it('缓冲不足如实抛错;连通域超上限提示调高 minArea', () => {
    expect(() => detectSpriteBoxes(8, 8, new Uint8ClampedArray(16))).toThrow(/像素缓冲不足/);
    // 4 个 2×2 块,minArea=1、maxBoxes=3 → 超限报错。
    const img = makeImage(16, 4, TRANSPARENT, [
      { x: 0, y: 0, w: 2, h: 2 },
      { x: 4, y: 0, w: 2, h: 2 },
      { x: 8, y: 0, w: 2, h: 2 },
      { x: 12, y: 0, w: 2, h: 2 },
    ]);
    expect(() => detectSpriteBoxes(16, 4, img, { minArea: 1, maxBoxes: 3 })).toThrow(/超上限/);
  });
});
