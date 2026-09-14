/**
 * F-GAME-4 前端本地自动切帧(纯函数,零往返即时预览):
 * 与 assetd sprite.rs autoslice_image **同规则**——
 * - 背景判定 = alpha<阈值(缺省 5)或品红族(2*g < min(r,b),与视口色键同规则);
 * - 4 连通 flood fill(迭代栈防深递归);
 * - 连通域像素数 < minArea(缺省 16)过滤噪点;
 * - 行带分组排序:按中心 y 分行(带高随行内帧扩展)、行间按 y、行内按 x。
 * 确定性:同图同参恒同序,与服务端 sprite_autoslice 结果一致。
 */

/** 紧致包围盒 [x, y, w, h](像素,y 向下图像空间)。 */
export type SpriteBox = [number, number, number, number];

export interface DetectOptions {
  /** alpha 低于此值视为背景(0-255,缺省 5 ≈ shader 的 0.02×255)。 */
  alphaThreshold?: number;
  /** 连通域像素数下限(缺省 16)。 */
  minArea?: number;
  /** 帧数上限保护(超限如实抛错提示调高 minArea,缺省 256)。 */
  maxBoxes?: number;
}

/** 背景判定:alpha 过低,或品红族(G 显著低于 R/B 两者:2*g < min(r,b))。 */
export function isBackgroundPixel(
  r: number,
  g: number,
  b: number,
  a: number,
  alphaThreshold = 5,
): boolean {
  if (a < alphaThreshold) return true;
  return 2 * g < Math.min(r, b);
}

/**
 * RGBA8 像素缓冲连通域检测 → 行带分组排序的 bbox 列表。
 * 缓冲不足/连通域超上限时抛 Error(消息与服务端语义对齐,调用方如实上屏)。
 */
export function detectSpriteBoxes(
  width: number,
  height: number,
  rgba: Uint8ClampedArray | Uint8Array,
  opts: DetectOptions = {},
): SpriteBox[] {
  const alphaThreshold = opts.alphaThreshold ?? 5;
  const minArea = opts.minArea ?? 16;
  const maxBoxes = opts.maxBoxes ?? 256;
  const total = width * height;
  if (rgba.length < total * 4) {
    throw new Error(
      `像素缓冲不足: ${width}x${height} 需 ${total * 4} 字节,实际 ${rgba.length}`,
    );
  }

  const visited = new Uint8Array(total);
  const found: Array<{ box: SpriteBox; count: number }> = [];

  for (let start = 0; start < total; start++) {
    if (visited[start]) continue;
    visited[start] = 1;
    const o = start * 4;
    if (isBackgroundPixel(rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3], alphaThreshold)) {
      continue;
    }
    // flood fill(4 连通,迭代栈)。
    let minX = start % width;
    let maxX = minX;
    let minY = Math.floor(start / width);
    let maxY = minY;
    let count = 0;
    const stack = [start];
    while (stack.length > 0) {
      const idx = stack.pop() as number;
      count += 1;
      const x = idx % width;
      const y = Math.floor(idx / width);
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
      const tryPush = (nidx: number) => {
        if (visited[nidx]) return;
        visited[nidx] = 1;
        const no = nidx * 4;
        if (!isBackgroundPixel(rgba[no], rgba[no + 1], rgba[no + 2], rgba[no + 3], alphaThreshold)) {
          stack.push(nidx);
        }
      };
      if (x > 0) tryPush(idx - 1);
      if (x + 1 < width) tryPush(idx + 1);
      if (y > 0) tryPush(idx - width);
      if (y + 1 < height) tryPush(idx + width);
    }
    if (count >= minArea) {
      found.push({ box: [minX, minY, maxX - minX + 1, maxY - minY + 1], count });
    }
  }

  if (found.length > maxBoxes) {
    throw new Error(
      `检出 ${found.length} 个连通域超上限 ${maxBoxes}(疑为噪点,建议调高 minArea,当前 ${minArea})`,
    );
  }

  // 行带分组:按中心 y(整除)+ x 预排 → 中心 y 落入当前行带则同行(带界随行内帧扩展);
  // 行间按 y、行内按 (x, y),得到"从上到下、行内从左到右"的自然帧序。
  const items = found.map((f) => f.box);
  items.sort((a, b) => a[1] + Math.floor(a[3] / 2) - (b[1] + Math.floor(b[3] / 2)) || a[0] - b[0]);
  const rows: Array<{ top: number; bottom: number; row: SpriteBox[] }> = [];
  for (const b of items) {
    const cy = b[1] + Math.floor(b[3] / 2);
    const last = rows[rows.length - 1];
    if (last && cy >= last.top && cy <= last.bottom) {
      last.top = Math.min(last.top, b[1]);
      last.bottom = Math.max(last.bottom, b[1] + b[3]);
      last.row.push(b);
    } else {
      rows.push({ top: b[1], bottom: b[1] + b[3], row: [b] });
    }
  }
  const out: SpriteBox[] = [];
  for (const r of rows) {
    r.row.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
    out.push(...r.row);
  }
  return out;
}
