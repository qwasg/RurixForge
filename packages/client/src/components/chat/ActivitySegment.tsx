import { useState } from 'react';
import {
  groupPhrase,
  groupSegmentItems,
  runningLabel,
  segmentPhrase,
  segmentStats,
  toolStatus,
  toolSummary,
  toolVisual,
  type ChatBlock,
} from '@/lib/timeline';
import { cn } from '@/lib/cn';

/**
 * F7 wave.4 活动段(参考 render_activity_segment / render_summary_line / render_tool_line):
 * 单行灰字汇总(12.5px text_3,运行中带脉冲点 +「· 正在{动词}…」),+N sage / -N danger /
 * 「· n 失败」danger;点击展开逐 ToolLine(组内 ≥2 同类并组为一行「创建 3 个实体」,
 * 可再展开);展开细节无边框 mono 11.5 text_4,args pretty JSON max-h 320,60 行截断
 * 「（+N 行已省略）」。
 */

function PulseDot() {
  return <span className="h-[6px] w-[6px] shrink-0 animate-pulse rounded-full bg-dot-running" />;
}

/** 灰字汇总行(参考 render_summary_line:text/+N/-N/· n 失败/脉冲点;可点击展开)。 */
export function SummaryLine({
  text,
  added = 0,
  removed = 0,
  errors = 0,
  running = false,
  expanded,
  onToggle,
  testId,
  children,
}: {
  text: string;
  added?: number;
  removed?: number;
  errors?: number;
  running?: boolean;
  expanded?: boolean;
  onToggle?: () => void;
  testId?: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex flex-col">
      <div
        data-testid={testId}
        onClick={onToggle}
        className={cn(
          'flex items-center gap-[5px] py-0.5 text-[12.5px]',
          errors > 0 ? 'text-danger' : 'text-fg-3',
          onToggle && 'cursor-pointer hover:text-fg-2',
        )}
      >
        <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">{text}</span>
        {added > 0 && <span className="shrink-0 text-[11.5px] text-sage">+{added}</span>}
        {removed > 0 && <span className="shrink-0 text-[11.5px] text-danger">-{removed}</span>}
        {errors > 0 && <span className="shrink-0 text-[11px] text-danger">· {errors} 失败</span>}
        {running && <PulseDot />}
      </div>
      {expanded && children && <div className="ml-2 pl-2 pb-1 pt-0.5">{children}</div>}
    </div>
  );
}

/** 工具详情体(参考 render_tool_body:args + 结果/错误,mono 11.5 text_4,60 行截断)。 */
function ToolDetailBody({ block }: { block: Extract<ChatBlock, { kind: 'tool' }> }) {
  const st = toolStatus(block);
  const boxed = (text: string, key: string) => {
    const lines = text.split('\n');
    const shown = lines.slice(0, 60);
    const omitted = lines.length - shown.length;
    return (
      <div
        key={key}
        data-testid={`tool-detail-${key}`}
        className="max-h-[320px] overflow-hidden font-code text-[11.5px] whitespace-pre-wrap text-fg-4"
      >
        {shown.map((l, i) => (
          <div key={i}>{l === '' ? ' ' : l}</div>
        ))}
        {omitted > 0 && <div className="text-fg-4">（+{omitted} 行已省略）</div>}
      </div>
    );
  };
  return (
    <div className="flex flex-col gap-1.5">
      {block.args !== '' && boxed(block.args, 'args')}
      {st === 'error'
        ? boxed(block.error ?? '', 'error')
        : st === 'running'
          ? <div className="text-[11.5px] text-fg-4">运行中…</div>
          : block.durationMs !== undefined
            ? <div className="text-[11.5px] text-fg-4">完成 · {block.durationMs}ms</div>
            : <div className="text-[11.5px] text-fg-4">无输出</div>}
    </div>
  );
}

/** 单工具行(参考 render_tool_line:「{动词} · {摘要}」,点击展开 args 体)。 */
export function ToolLine({ block }: { block: Extract<ChatBlock, { kind: 'tool' }> }) {
  const [open, setOpen] = useState(false);
  const st = toolStatus(block);
  const summary = toolSummary(block);
  const line = summary === '' ? toolVisual(block.name) : `${toolVisual(block.name)} · ${summary}`;
  return (
    <SummaryLine
      text={line}
      errors={st === 'error' ? 1 : 0}
      running={st === 'running'}
      expanded={open}
      onToggle={() => setOpen((v) => !v)}
      testId={`tool-line-${block.toolCallId}`}
    >
      <ToolDetailBody block={block} />
    </SummaryLine>
  );
}

/** 活动段汇总行 + 展开的工具行列表(组内同类 ≥2 并组)。 */
export default function ActivitySegment({
  blocks,
  indices,
}: {
  blocks: ChatBlock[];
  indices: number[];
}) {
  const [open, setOpen] = useState(false);
  const stats = segmentStats(blocks, indices);
  const running = runningLabel(blocks, indices);
  const text = running ? `${segmentPhrase(stats)} · ${running}` : segmentPhrase(stats);

  const runs = groupSegmentItems(blocks, indices);
  return (
    <SummaryLine
      text={text}
      added={stats.added}
      removed={stats.removed}
      errors={stats.errors}
      running={running !== null}
      expanded={open}
      onToggle={() => setOpen((v) => !v)}
      testId="activity-segment"
    >
      <div className="flex flex-col">
        {runs.map((run, ri) => {
          const first = blocks[run[0]];
          if (run.length >= 2 && first.kind === 'tool') {
            return <GroupedRun key={ri} blocks={blocks} run={run} />;
          }
          return first.kind === 'tool' ? <ToolLine key={ri} block={first} /> : null;
        })}
      </div>
    </SummaryLine>
  );
}

/** 同类并组行(参考 render_activity_segment 内 run.len>=2 分支:「读取 5 个文件」可再展开)。 */
function GroupedRun({ blocks, run }: { blocks: ChatBlock[]; run: number[] }) {
  const [open, setOpen] = useState(false);
  const first = blocks[run[0]] as Extract<ChatBlock, { kind: 'tool' }>;
  const errors = run.filter((bi) => {
    const b = blocks[bi];
    return b.kind === 'tool' && toolStatus(b) === 'error';
  }).length;
  return (
    <SummaryLine
      text={groupPhrase(first.name, run.length)}
      errors={errors}
      expanded={open}
      onToggle={() => setOpen((v) => !v)}
      testId="activity-group"
    >
      <div className="flex flex-col">
        {run.map((bi) => {
          const b = blocks[bi];
          return b.kind === 'tool' ? <ToolLine key={bi} block={b} /> : null;
        })}
      </div>
    </SummaryLine>
  );
}
