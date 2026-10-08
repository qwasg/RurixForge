export interface QuotaWindow {
  id: string;
  label: string;
  usedPercent: number;
  windowDurationMins?: number;
  resetsAt?: number | string;
}

function record(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown> : undefined;
}

/** Use official Codex windows, preferring the complete map over the legacy summary. */
export function codexQuotaWindows(raw: unknown, planType?: string | null): QuotaWindow[] {
  const root = record(raw);
  const value = record(root?.rateLimits) ?? root;
  if (!value) return [];
  const byId = record(value.rateLimitsByLimitId);
  const groups: [string, unknown][] = byId && Object.keys(byId).length > 0
    ? Object.entries(byId) : [[typeof value.limitId === 'string' ? value.limitId : '', value]];
  return groups
    // The base-model reserve is separate from the Codex coding allowance.
    .filter(([id]) => id !== 'base_model_inference')
    .sort(([a], [b]) => Number(b === 'codex') - Number(a === 'codex'))
    .flatMap(([groupId, rawGroup]) => {
      const group = record(rawGroup);
      if (!group) return [];
      const plan = planType ?? group.planType ?? value.planType ?? root?.planType;
      const pro = typeof plan === 'string' && /^pro(?:$|[-_\s])/i.test(plan.trim());
      return ['primary', 'secondary'].flatMap((key) => {
        const bucket = record(group[key] ?? (key === 'primary' ? group : undefined));
        if (!bucket) return [];
        const used = bucket.usedPercent ?? bucket.used_percent;
        if (typeof used !== 'number' || !Number.isFinite(used)) return [];
        const mins = bucket.windowDurationMins ?? bucket.window_duration_mins;
        const duration = typeof mins === 'number' && Number.isFinite(mins) && mins > 0 ? mins : undefined;
        // Pro has no five-hour limit; cached pre-upgrade buckets must not reintroduce it.
        if (pro && duration === 300) return [];
        const label = duration === 10080 ? '周额度'
          : duration !== undefined ? duration >= 1440 ? `${duration / 1440} 天` : `${duration / 60} 小时`
          : key === 'primary' ? '主要窗口' : '次要窗口';
        const reset = bucket.resetsAt ?? bucket.resets_at;
        return [{
          id: groupId ? `${groupId}-${key}` : key,
          label: groupId && groupId !== 'codex' ? `${groupId} · ${label}` : label,
          usedPercent: Math.max(0, Math.min(100, used)),
          windowDurationMins: duration,
          resetsAt: typeof reset === 'number' || typeof reset === 'string' ? reset : undefined,
        }];
      });
    });
}
