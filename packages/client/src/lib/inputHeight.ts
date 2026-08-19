/**
 * F7 wave.4 输入框自适应高(参考 app.rs estimate_wrapped_lines / auto_input_height 逐行移植):
 * 按 44 列估行,高 = min + (n-1)×20,clamp [min,max];composer 用 [68,200](底 32),
 * 内联编辑用 [56,200](底 56)。
 */

/** 参考 estimate_wrapped_lines:逐 \n 分段,chars/44 上取整,空段计 1。 */
export function estimateWrappedLines(text: string, wrapCols = 44): number {
  let lines = 0;
  for (const raw of text.split('\n')) {
    const chars = [...raw].length;
    lines += Math.floor(chars / wrapCols) + (chars % wrapCols !== 0 ? 1 : 0);
    if (chars === 0) lines += 1;
  }
  return Math.max(lines, 1);
}

/** 参考 auto_input_height:(min-h 基准 + (n-1)*20) clamp [min, max]。 */
export function autoInputHeight(text: string, base: number, minH: number, maxH: number): number {
  const n = estimateWrappedLines(text, 44);
  return Math.min(Math.max(base + (n - 1) * 20, minH), maxH);
}

/** composer 输入壳高:32+(n-1)×20 clamp 68–200。 */
export function composerInputHeight(text: string): number {
  return autoInputHeight(text, 32, 68, 200);
}

/** 用户卡内联编辑高:56+(n-1)×20 clamp 56–200。 */
export function editInputHeight(text: string): number {
  return autoInputHeight(text, 56, 56, 200);
}
