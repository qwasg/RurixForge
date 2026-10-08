import { useMemo, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react';
import type { UsageDay } from '@/lib/accountApi';
import { activityLevel, buildUsageActivity, type ActivityCell, type ActivityMode } from '@/lib/usageActivity';
import './usageActivity.css';

const modes = [['daily', '每天'], ['weekly', '每周'], ['total', '累计总量']] as const;
const numberFmt = new Intl.NumberFormat('en-US');

function cellLabel(cell: ActivityCell, mode: ActivityMode): string {
  const value = `${numberFmt.format(cell.value)} Token`;
  if (mode === 'weekly') return `${cell.date} — ${cell.endDate} · 本周 ${value}`;
  if (mode === 'total') return `截至 ${cell.date} · 近一年累计 ${value}`;
  return `${cell.date} · ${value} · 输入 ${numberFmt.format(cell.day.inputTokens)} / 输出 ${numberFmt.format(cell.day.outputTokens)} · ${numberFmt.format(cell.day.requests)} 次调用`;
}

export default function UsageActivity({ items, today }: { items: UsageDay[]; today: Date }) {
  const [mode, setMode] = useState<ActivityMode>('daily');
  const [focusedDate, setFocusedDate] = useState<string | null>(null);
  const [selectedDate, setSelectedDate] = useState<string | null>(null);
  const buttons = useRef(new Map<string, HTMLButtonElement>());
  const calendar = useMemo(() => buildUsageActivity(items, mode, today), [items, mode, today]);
  const max = Math.max(0, ...calendar.cells.map((cell) => cell.value));
  const selected = calendar.cells.find((cell) => cell.date === selectedDate);
  const tabDate = calendar.cells.some((cell) => cell.date === focusedDate)
    ? focusedDate : calendar.cells[calendar.cells.length - 1].date;
  const style = { '--activity-columns': calendar.columns } as CSSProperties;

  const moveFocus = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const jumps: Record<string, number> = { ArrowLeft: mode === 'weekly' ? -1 : -7,
      ArrowRight: mode === 'weekly' ? 1 : 7, ArrowUp: -1, ArrowDown: 1 };
    const target = event.key === 'Home' ? 0 : event.key === 'End' ? calendar.cells.length - 1
      : event.key in jumps ? Math.min(calendar.cells.length - 1, Math.max(0, index + jumps[event.key])) : null;
    if (target === null) return;
    event.preventDefault();
    buttons.current.get(calendar.cells[target].date)?.focus();
  };

  return (
    <section data-testid="account-usage-activity" className="usage-activity" aria-label="Token 活动">
      <div className="usage-activity-header">
        <span className="usage-activity-title">Token 活动</span>
        <div className="usage-activity-modes" role="group" aria-label="Token 活动显示方式">
          {modes.map(([id, label]) => (
            <button key={id} type="button" aria-pressed={mode === id} onClick={() => {
              setMode(id); setFocusedDate(null); setSelectedDate(null);
            }}>{label}</button>
          ))}
        </div>
      </div>
      <div className="usage-activity-scroll">
        <div className="usage-activity-calendar" style={style}>
          <div className="usage-activity-grid" role="group" aria-label={`近一年${modes.find(([id]) => id === mode)![1]} Token 日期热力图`}>
            {mode === 'weekly' && Array.from({ length: calendar.columns * 7 }, (_, index) => (
              <span key={index} className="usage-activity-spacer" aria-hidden="true"
                style={{ gridColumn: Math.floor(index / 7) + 1, gridRow: index % 7 + 1 }} />
            ))}
            {calendar.cells.map((cell, index) => (
              <button key={cell.date} type="button" title={cellLabel(cell, mode)} aria-label={cellLabel(cell, mode)}
                data-testid={`account-activity-${cell.date}`} data-level={activityLevel(cell.value, max)}
                className={`usage-activity-cell${mode === 'weekly' ? ' usage-activity-week' : ''}`}
                style={{ gridColumn: cell.week + 1, gridRow: mode === 'weekly' ? '1 / span 7' : cell.row + 1 }}
                tabIndex={cell.date === tabDate ? 0 : -1} aria-pressed={selectedDate === cell.date}
                ref={(node) => { if (node) buttons.current.set(cell.date, node); else buttons.current.delete(cell.date); }}
                onFocus={() => setFocusedDate(cell.date)} onKeyDown={(event) => moveFocus(event, index)}
                onClick={() => setSelectedDate(cell.date)} />
            ))}
          </div>
          <div className="usage-activity-months" aria-hidden="true">
            {calendar.months.map((month) => <span key={month.date} style={{ gridColumn: `${month.week + 1} / span ${Math.min(3, calendar.columns - month.week)}` }}>{month.label}</span>)}
          </div>
        </div>
      </div>
      <div className="usage-activity-footer">
        <span title={`${calendar.firstDate} — ${calendar.lastDate} · UTC`}>近一年 · {numberFmt.format(calendar.total)} Token</span>
        <div className="usage-activity-legend" aria-label="颜色越深表示 Token 用量越多">
          <span>少</span>{[0, 1, 2, 3, 4].map((level) => <i key={level} data-level={level} aria-hidden="true" />)}<span>多</span>
        </div>
      </div>
      {selected && <div className="usage-activity-detail" role="status">{cellLabel(selected, mode)}</div>}
    </section>
  );
}
