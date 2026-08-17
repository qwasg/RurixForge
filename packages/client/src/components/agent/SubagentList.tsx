import { useEffect, useState } from 'react';
import { ArrowUpRight, ChevronDown, Sparkles, X } from 'lucide-react';
import type { SubagentRun } from '@/lib/types';
import { cn } from '@/lib/cn';
import Markdown from './Markdown';

export type SubagentPhase = 'starting' | 'running' | 'done';

interface SubagentListProps {
  items: SubagentRun[];
  phases: Record<string, SubagentPhase>;
  onStop: (id: string) => void;
}

const iconBtn =
  'grid h-6 w-6 place-items-center rounded-md text-muted transition-colors hover:bg-panel-hover hover:text-ink-soft';

const stopBtn =
  'rounded-full bg-panel-hover px-2.5 py-1 text-xs text-muted transition-colors hover:bg-panel-active hover:text-ink-soft';

/** 聚合类活动行(Thought / Explored / Worked …)带可旋转 chevron,对齐视频。 */
const COLLAPSIBLE_RE = /^(Thought|Explor|Worked|Updated)/;

function ActivityLine({ text }: { text: string }) {
  const collapsible = COLLAPSIBLE_RE.test(text);
  const [open, setOpen] = useState(false);
  return (
    <button
      type="button"
      disabled={!collapsible}
      onClick={() => setOpen((o) => !o)}
      className={cn(
        'flex w-full items-center gap-1 text-left text-xs text-muted',
        collapsible ? 'hover:text-ink-soft' : 'cursor-default',
      )}
    >
      <span className="min-w-0">{text}</span>
      {collapsible && (
        <ChevronDown
          size={11}
          className={cn('shrink-0 text-muted-faint transition-transform', open && 'rotate-180')}
        />
      )}
    </button>
  );
}

interface SubagentRowProps {
  run: SubagentRun;
  phase: SubagentPhase;
  expanded: boolean;
  onToggle: () => void;
  onStop: (id: string) => void;
}

function SubagentRow({ run, phase, expanded, onToggle, onStop }: SubagentRowProps) {
  const running = phase === 'running';
  // 运行期间按 statusSequence 每 2s 轮播,停在最后一条
  const [step, setStep] = useState(0);
  useEffect(() => {
    if (!running) return;
    const iv = window.setInterval(
      () => setStep((s) => Math.min(s + 1, run.statusSequence.length - 1)),
      2000,
    );
    return () => window.clearInterval(iv);
  }, [running, run.statusSequence.length]);

  const status =
    phase === 'done'
      ? run.doneLabel
      : run.statusSequence[Math.min(step, run.statusSequence.length - 1)];

  if (expanded) {
    return (
      <div className="rounded-2xl border border-line bg-white">
        <div className="flex items-center gap-2 px-4 pt-3">
          <span className="text-sm font-semibold text-ink">{run.title}</span>
          <div className="ml-auto flex items-center gap-0.5">
            <button type="button" className={iconBtn} title="Open">
              <ArrowUpRight size={14} />
            </button>
            <button type="button" className={iconBtn} onClick={onToggle} title="Close">
              <X size={14} />
            </button>
          </div>
        </div>
        <div className="px-4 pb-4 pt-2">
          <div className="whitespace-pre-wrap rounded-xl bg-panel px-3 py-2.5 text-sm text-ink-soft">
            {run.prompt}
          </div>
          <div className="relative mt-3 space-y-1.5">
            {running && (
              <button type="button" onClick={() => onStop(run.id)} className={cn(stopBtn, 'absolute right-0 top-0')}>
                Stop
              </button>
            )}
            {run.activity.map((line, i) => (
              <ActivityLine key={i} text={line} />
            ))}
          </div>
          {phase === 'done' && run.report && (
            <div className="mt-4 border-t border-line-soft pt-4">
              <Markdown md={run.report} />
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div
      onClick={onToggle}
      className="group cursor-pointer rounded-lg px-2 py-1.5 transition-colors hover:bg-panel-hover"
    >
      <div className="flex items-center gap-2">
        {running ? (
          <Sparkles size={13} className="shrink-0 text-ink-soft" />
        ) : (
          <span className="flex w-[13px] shrink-0 justify-center">
            <span className="h-1.5 w-1.5 rounded-full bg-muted-faint" />
          </span>
        )}
        <span className="text-sm font-medium text-ink">{run.title}</span>
        <span className="rounded-full bg-panel-hover px-1.5 py-0.5 text-2xs text-muted transition-colors group-hover:bg-panel-active">
          {run.badge}
        </span>
        {running && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onStop(run.id);
            }}
            className={cn(stopBtn, 'ml-auto opacity-0 transition-opacity group-hover:opacity-100')}
          >
            Stop
          </button>
        )}
      </div>
      <div className="pl-[21px] text-xs text-muted">{status}</div>
    </div>
  );
}

export default function SubagentList({ items, phases, onStop }: SubagentListProps) {
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const allStarting = items.every((it) => phases[it.id] !== 'running' && phases[it.id] !== 'done');

  // 启动阶段:单行 "New subagent / Starting up"(视频 2 00:00)
  if (allStarting) {
    return (
      <div className="px-2 py-1.5">
        <div className="flex items-center gap-2">
          <Sparkles size={13} className="text-ink-soft" />
          <span className="text-sm font-medium text-ink">New subagent</span>
        </div>
        <div className="pl-[21px] text-xs text-muted">Starting up</div>
      </div>
    );
  }

  return (
    <div className="space-y-0.5">
      {items.map((it) => (
        <SubagentRow
          key={it.id}
          run={it}
          phase={phases[it.id] ?? 'starting'}
          expanded={expandedId === it.id}
          onToggle={() => setExpandedId((cur) => (cur === it.id ? null : it.id))}
          onStop={onStop}
        />
      ))}
    </div>
  );
}
