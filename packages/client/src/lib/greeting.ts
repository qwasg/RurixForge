/** 首页时段问候(本地时间小时 0–23)。 */
export function greetingFor(hour: number): string {
  if (hour < 5) return '夜深了';
  if (hour < 11) return '早上好';
  if (hour < 13) return '中午好';
  if (hour < 18) return '下午好';
  return '晚上好';
}
