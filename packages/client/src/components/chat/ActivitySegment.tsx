import { useState, type ReactNode } from 'react';
import { ChevronDown } from 'lucide-react';
import {
  groupPhraseParts,
  groupSegmentItems,
  reasoningDurationMs,
  runningLabel,
  segmentPhraseParts,
  segmentStats,
  thinkingParts,
  toolStatus,
  toolTarget,
  toolVerb,
  type ChatBlock,
} from '@/lib/timeline';
import { cn } from '@/lib/cn';
import CommandBlock from './CommandBlock';
import FileChangeBlock from './FileChangeBlock';
import StreamEnter, { useFreshKeys } from './StreamEnter';

/**
 * F7 wave.4 活动段(参考 render_activity_segment / render_summary_line / render_tool_line)。
 *
 * 2026-09-03 用户指令(过程链英文 + 呈现目标截图效果)重做行形态,留痕:
 * - 每行两段式「{动词} {目标}」——动词 text_2 深一档、目标 text_4 浅一档,与加粗黑正文三级分明;
 *   汇总行 =「Exploring 12 files, 9 searches, ran 2 commands」+ chevron,展开后逐条明细:
 *   `Read timeline.ts L90-625` / `Grepped danger|dot-blocked in theme.css` /
 *   `Searched files <glob 串>` / `Ran cargo test` / `Thought 47s`。
 * - 思考行并进本段(timeline.isMilestoneBlock 不再拿 reasoning 断段):完成态「Thought {时长}」
 *   可展开全文;进行中「Thinking」渐变扫光,也可展开看实时思考流。
 * - 段内只有思考块(无工具)时不套汇总行,裸行直出——截图里正文上方那条「Thought briefly」即此形态。
 * - 报错不特别标明(用户指令):汇总行与工具行不再出红字、不再缀「· n 失败」,
 *   错误原文只在展开详情里如实可见;+N/-N 一并降为浅灰(不借语义色喊话)。
 *
 * D-047(Cursor 式运行态):「正在执行」的行整行扫光(.forge-shimmer)——段内有工具在跑的汇总行、
 * 运行中的工具行 / 命令块 / 文件变更块、思考中的 Thinking;一律以所在消息仍在流式为前提,
 * 轮次已收束却残留 running 的块不扫。展开态下新到的明细行自上而下入场(StreamEnter)。
 */

/** 两段式灰行(动词 + 目标 + 可选 +N/-N + 可选 chevron;点击展开 children)。 */
export function SummaryLine({
  verb,
  detail = '',
  shimmer = false,
  added = 0,
  removed = 0,
  chevron = false,
  expanded,
  onToggle,
  testId,
  children,
}: {
  verb: string;
  detail?: string;
  /**
   * D-047 正在执行:文字段整段扫光。扫光挂在贴字宽的内层(挂整行亮带大半时间在扫空白),
   * 期间动词 / 目标统一取扫光底色(text_2),收尾后回两档灰。
   */
  shimmer?: boolean;
  added?: number;
  removed?: number;
  chevron?: boolean;
  expanded?: boolean;
  onToggle?: () => void;
  testId?: string;
  children?: ReactNode;
}) {
  return (
    <div className="flex flex-col">
      <div
        data-testid={testId}
        onClick={onToggle}
        className={cn(
          'group/line flex items-center gap-[5px] py-0.5 text-[12.5px]',
          onToggle && 'cursor-pointer',
        )}
      >
        <span className={cn('flex min-w-0 items-center gap-[5px]', shimmer ? 'forge-shimmer' : 'text-fg-4')}>
          {/* 动词后带一个空格文本节点:flex 行尾空白不渲染(视距仍由 gap 决定),
              但复制整行与 textContent 取值时不会把「Read」「timeline.ts」黏成一坨。 */}
          <span className={cn('shrink-0', !shimmer && 'text-fg-2')}>{verb}{' '}</span>
          {detail !== '' && (
            <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">{detail}</span>
          )}
          {added > 0 && <span className="shrink-0 text-[11.5px]">+{added}</span>}
          {removed > 0 && <span className="shrink-0 text-[11.5px]">-{removed}</span>}
        </span>
        {chevron && onToggle && (
          <ChevronDown
            size={11}
            className={cn(
              'shrink-0 text-fg-4 transition-all duration-150',
              expanded ? 'rotate-180 opacity-70' : 'opacity-0 group-hover/line:opacity-70',
            )}
          />
        )}
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
        {omitted > 0 && <div className="text-fg-4">(+{omitted} lines omitted)</div>}
      </div>
    );
  };
  return (
    <div className="flex flex-col gap-1.5">
      {block.args !== '' && boxed(block.args, 'args')}
      {st === 'error'
        ? boxed(block.error ?? '', 'error')
        : st === 'running'
          ? <div className="text-[11.5px] text-fg-4">Running…</div>
          : (
            <>
              {block.result ? boxed(block.result, 'result') : null}
              {block.durationMs !== undefined
                ? <div className="text-[11.5px] text-fg-4">Done · {block.durationMs}ms</div>
                : !block.result
                  ? <div className="text-[11.5px] text-fg-4">No output</div>
                  : null}
            </>
          )}
    </div>
  );
}

/**
 * 单工具行「{动词} {目标}」,点击展开 args/结果/错误体。
 * streaming = 所在消息仍在流式:此时运行中的工具行(含命令 / 文件变更块)扫光。
 */
export function ToolLine({
  block,
  streaming = false,
}: {
  block: Extract<ChatBlock, { kind: 'tool' }>;
  streaming?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const live = streaming && toolStatus(block) === 'running';
  if (block.toolKind === 'command') return <CommandBlock block={block} live={live} />;
  if (block.toolKind === 'fileChange') return <FileChangeBlock block={block} live={live} />;
  return (
    <SummaryLine
      verb={toolVerb(block)}
      detail={toolTarget(block)}
      shimmer={live}
      expanded={open}
      onToggle={() => setOpen((v) => !v)}
      testId={`tool-line-${block.toolCallId}`}
    >
      <ToolDetailBody block={block} />
    </SummaryLine>
  );
}

/**
 * 思考行:进行中 = 扫光「Thinking」(默认收起不剧透,点开可看实时思考流);
 * 结束 = 「Thought {时长}」,点击展开 12px text_4 全文。
 */
export function ReasoningLine({
  block,
  live = false,
}: {
  block: Extract<ChatBlock, { kind: 'reasoning' }>;
  live?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const parts = live
    ? { verb: 'Thinking', detail: '' }
    : thinkingParts(reasoningDurationMs(block));
  return (
    <SummaryLine
      verb={parts.verb}
      detail={parts.detail}
      shimmer={live}
      chevron
      expanded={open}
      onToggle={() => setOpen((v) => !v)}
      testId="reasoning-line"
    >
      <div className="whitespace-pre-wrap text-[12px] text-fg-4">{block.text}</div>
    </SummaryLine>
  );
}

/**
 * 活动段:汇总行 + 展开的逐条明细(同类 ≥2 并组;思考块与工具行同列)。
 * 段内无工具(纯思考)时不套汇总行,裸行直出。
 */
export default function ActivitySegment({
  blocks,
  indices,
  streaming = false,
}: {
  blocks: ChatBlock[];
  indices: number[];
  streaming?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const stats = segmentStats(blocks, indices);
  const running = runningLabel(blocks, indices) !== null;
  const { verb, detail } = segmentPhraseParts(stats, running);
  const runs = groupSegmentItems(blocks, indices);
  const hasTool = indices.some((bi) => blocks[bi].kind === 'tool');
  // 段本身的入场由外层(AssistantMessage)负责;这里只让段挂上之后才到的明细行各自入场
  const isFresh = useFreshKeys(indices);

  const items = (
    <div className="flex flex-col">
      {runs.map((run, ri) => {
        const first = blocks[run[0]];
        let row: ReactNode = null;
        if (run.length >= 2 && first.kind === 'tool') {
          row = <GroupedRun blocks={blocks} run={run} />;
        } else if (first.kind === 'tool') {
          row = <ToolLine block={first} streaming={streaming} />;
        } else if (first.kind === 'reasoning') {
          row = <ReasoningLine block={first} live={streaming && run[0] === blocks.length - 1} />;
        }
        if (row === null) return null;
        return (
          <StreamEnter key={ri} active={streaming && isFresh(run[0])}>
            {row}
          </StreamEnter>
        );
      })}
    </div>
  );

  // 纯思考段:截图里正文上方那条独立「Thought briefly」——不套「Working」汇总壳。
  if (!hasTool) return items;

  return (
    <SummaryLine
      verb={verb}
      detail={detail}
      shimmer={streaming && running}
      added={stats.added}
      removed={stats.removed}
      chevron
      expanded={open}
      onToggle={() => setOpen((v) => !v)}
      testId="activity-segment"
    >
      {items}
    </SummaryLine>
  );
}

/** 同类并组行(参考 render_activity_segment 内 run.len>=2 分支:「Read 5 files」可再展开)。 */
function GroupedRun({ blocks, run }: { blocks: ChatBlock[]; run: number[] }) {
  const [open, setOpen] = useState(false);
  const first = blocks[run[0]] as Extract<ChatBlock, { kind: 'tool' }>;
  const { verb, detail } = groupPhraseParts(first.name, run.length);
  return (
    <SummaryLine
      verb={verb}
      detail={detail}
      chevron
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
