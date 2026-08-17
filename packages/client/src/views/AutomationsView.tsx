import { useMemo, useState } from 'react';
import {
  Activity,
  ArrowRight,
  BarChart3,
  Bug,
  Clock,
  FlaskConical,
  GitPullRequest,
  HeartPulse,
  Inbox,
  MessageSquare,
  Plus,
  Search,
  ShieldAlert,
  TriangleAlert,
  Wrench,
  Zap,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { AUTOMATION_CATEGORIES, AUTOMATION_TEMPLATES } from '@/lib/mock';
import { cn } from '@/lib/cn';

/** 模板卡左上角圆形小图标(录屏中每张卡图标不同,按 id 映射)。 */
const TEMPLATE_ICONS: Record<string, LucideIcon> = {
  t1: Bug,
  t2: ShieldAlert,
  t3: FlaskConical,
  t4: Activity,
  t5: HeartPulse,
  t6: Wrench,
  t7: GitPullRequest,
  t8: BarChart3,
  t9: TriangleAlert,
  t10: Inbox,
};

const STATS = [
  { label: 'Total Automations', value: '0' },
  { label: 'Successful · 7d', value: '0' },
  { label: 'Failed · 7d', value: '0' },
];

/** Automations 页:标题 + 统计卡 + Mine/Team + 空态卡 + 分类 chips + 模板卡网格。 */
export default function AutomationsView() {
  const [scope, setScope] = useState<'mine' | 'team'>('mine');
  const [query, setQuery] = useState('');
  const [category, setCategory] = useState<string | null>(null);

  const templates = useMemo(() => {
    const q = query.trim().toLowerCase();
    return AUTOMATION_TEMPLATES.filter((t) => {
      if (category && t.category !== category) return false;
      if (q && !t.title.toLowerCase().includes(q) && !t.description.toLowerCase().includes(q)) {
        return false;
      }
      return true;
    });
  }, [category, query]);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-[880px] px-8 pb-16 pt-10">
        {/* 标题行 */}
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="text-2xl font-semibold text-ink">Automations</h1>
            <p className="mt-1 text-sm text-muted">
              Automate repetitive tasks with always-on cloud agents that respond to environment
              triggers.
            </p>
          </div>
          <button type="button" className="btn-primary mt-0.5 shrink-0">
            <Plus size={13} strokeWidth={2} />
            New Automation
          </button>
        </div>

        {/* 统计卡 */}
        <div className="mt-6 grid grid-cols-4 gap-3">
          {STATS.map((s) => (
            <div key={s.label} className="rounded-xl border border-line p-4">
              <div className="text-xs text-muted">{s.label}</div>
              <div className="mt-1 text-xl font-semibold text-ink">{s.value}</div>
            </div>
          ))}
          <button
            type="button"
            className="rounded-xl border border-line p-4 text-left transition-colors hover:bg-panel"
          >
            <span className="inline-flex items-center gap-1 text-xs text-muted">
              Run History
              <ArrowRight size={12} />
            </span>
          </button>
        </div>

        {/* Mine/Team + 搜索 */}
        <div className="mt-8 flex items-center justify-between gap-4">
          <div className="flex items-center gap-1">
            {(['mine', 'team'] as const).map((s) => (
              <button
                key={s}
                type="button"
                onClick={() => setScope(s)}
                className={cn(
                  'rounded-full px-2.5 py-1 text-xs transition-colors',
                  scope === s
                    ? 'bg-panel-active font-medium text-ink'
                    : 'text-muted hover:bg-panel-hover',
                )}
              >
                {s === 'mine' ? 'Mine' : 'Team'}
              </button>
            ))}
          </div>
          <div className="relative w-52">
            <Search
              size={13}
              className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-faint"
            />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === 'Escape' && setQuery('')}
              placeholder="Search..."
              className="w-full rounded-lg border border-transparent bg-panel py-1.5 pl-8 pr-3 text-xs text-ink outline-none transition-colors placeholder:text-muted-faint focus:border-line focus:bg-white"
            />
          </div>
        </div>

        {/* 空态卡 */}
        <div className="mt-4 flex flex-col items-center rounded-xl border border-line py-10 text-center">
          <p className="text-sm font-semibold text-ink">No Automations Yet</p>
          <p className="mt-2 max-w-[360px] text-xs leading-5 text-muted">
            Run agents on a schedule or automatically in response to events. Billed at{' '}
            <span className="underline underline-offset-2">plan rates.</span>
          </p>
          <button type="button" className="btn-ghost mt-4">
            Add Automation
          </button>
        </div>

        {/* 分类 chips */}
        <div className="mt-8 flex flex-wrap items-center gap-2">
          {AUTOMATION_CATEGORIES.map((c) => (
            <button
              key={c}
              type="button"
              onClick={() => setCategory((cur) => (cur === c ? null : c))}
              className={cn(
                'rounded-full border px-3 py-1 text-xs transition-colors',
                category === c
                  ? 'border-ink bg-ink text-white'
                  : 'border-line bg-white text-ink-soft hover:bg-panel-hover',
              )}
            >
              {c}
            </button>
          ))}
        </div>

        {/* 模板卡两列网格 */}
        <div className="mt-4 grid grid-cols-2 gap-3">
          {templates.map((t) => {
            const Icon = TEMPLATE_ICONS[t.id] ?? Zap;
            const TriggerIcon = t.trigger === 'Scheduled' ? Clock : Zap;
            return (
              <button
                key={t.id}
                type="button"
                className="flex flex-col rounded-xl border border-line p-4 text-left transition-colors hover:bg-panel"
              >
                <div className="flex items-center gap-2">
                  <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-panel-hover text-muted">
                    <Icon size={14} strokeWidth={1.75} />
                  </span>
                  <span className="text-sm font-semibold text-ink">{t.title}</span>
                </div>
                <p className="mt-2 line-clamp-2 text-xs leading-5 text-muted">{t.description}</p>
                <div className="mt-auto flex items-center gap-1.5 pt-3 text-2xs text-muted-faint">
                  <TriggerIcon size={11} />
                  <span>{t.trigger}</span>
                  <ArrowRight size={11} />
                  <MessageSquare size={11} />
                  <span>{t.action}</span>
                </div>
              </button>
            );
          })}
        </div>
        {templates.length === 0 && (
          <p className="py-10 text-center text-xs text-muted-faint">No templates found</p>
        )}
      </div>
    </div>
  );
}
