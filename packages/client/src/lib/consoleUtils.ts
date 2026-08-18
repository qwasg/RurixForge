/**
 * Console 面板纯函数(F6 wave.3):级别分类 / 类型过滤 / 清空标记 / 采样环追加。
 * 全部无副作用,供 EditorView 渲染层与 vitest 复用。
 */

/** Console 行级别:error(call_error/unsupported/工具失败/报告失败行)> playtest(报告注入)> info。 */
export type ConsoleLevel = 'error' | 'playtest' | 'info';

/** 事件类型名(event 字段;缺失退 'event')。 */
export function eventType(e: Record<string, unknown>): string {
  return typeof e.event === 'string' ? e.event : 'event';
}

/** 级别判定:playtest 注入行(role=playtest)优先;名称含 error/unsupported 或 ok=false 报错误级。 */
export function consoleLevel(e: Record<string, unknown>): ConsoleLevel {
  if (e.role === 'playtest') return 'playtest';
  const t = eventType(e);
  if (t.includes('error') || t.includes('unsupported')) return 'error';
  if (e.ok === false) return 'error';
  return 'info';
}

/** 视图过滤:剔除隐藏类型 + 清空标记(clearedBefore = 本地视图态下标,之前行不显示)。 */
export function filterEvents(
  events: Array<Record<string, unknown>>,
  hidden: ReadonlySet<string>,
  clearedBefore: number,
): Array<Record<string, unknown>> {
  return events.slice(Math.max(0, clearedBefore)).filter((e) => !hidden.has(eventType(e)));
}

/** 出现类型清单(按首现序,附计数)。 */
export function typeCounts(events: Array<Record<string, unknown>>): Array<{ type: string; count: number }> {
  const m = new Map<string, number>();
  for (const e of events) {
    const t = eventType(e);
    m.set(t, (m.get(t) ?? 0) + 1);
  }
  return [...m.entries()].map(([type, count]) => ({ type, count }));
}

/** 定长采样环追加(返回新数组;cap 默认 60)。 */
export function ringPush<T>(ring: readonly T[], v: T, cap = 60): T[] {
  const next = [...ring, v];
  return next.length > cap ? next.slice(next.length - cap) : next;
}
