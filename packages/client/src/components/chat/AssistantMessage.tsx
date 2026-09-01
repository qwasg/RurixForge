import { useState } from 'react';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import {
  buildTimeline,
  reasoningSummary,
  todoMilestoneLabel,
  type ChatBlock,
} from '@/lib/timeline';
import MarkdownFlat from './MarkdownFlat';
import ActivitySegment, { SummaryLine, ToolLine } from './ActivitySegment';
import SubagentRow from './SubagentRow';
import StreamCaret from './StreamCaret';
import { StatusDot } from '../shell/primitives';

/**
 * F7 wave.4 助手消息(参考 render_agent_message):
 * 头 = 22×22 圆角 6「铸」方块(bg=text text_inv serif 11px,替参考「月」)+「Agent」
 * + 状态点(streaming 脉冲/completed sage/failed danger/cancelled 灰)+ 模型 label + 右侧时间。
 * blocks 经 buildTimeline:末 text 块=最终回答 MarkdownFlat 全量不折叠;中间 text 块
 * =text_2 13px 内联;tool 段=ActivitySegment;write_todos 里程碑=「待办 · …」折叠行;
 * reasoning=「思考 · {摘录} · {N} 字」折叠行(本仓事件面不产生,组件就绪);
 * subagent=SubagentRow。streaming 时消息末挂 StreamCaret。
 */
export default function AssistantMessage({ msg }: { msg: ChatMsg }) {
  const streaming = msg.status === 'streaming';
  const dotColor =
    msg.status === 'completed'
      ? 'var(--dot-done)'
      : msg.status === 'failed'
        ? 'var(--dot-blocked)'
        : msg.status === 'cancelled'
          ? 'var(--dot-idle)'
          : 'var(--dot-running)';

  const blocks = msg.blocks;
  const finalTextIdx = blocks.map((b) => b.kind).lastIndexOf('text');
  const timeline = buildTimeline(blocks);

  return (
    <div data-testid="assistant-message" className="flex flex-col gap-1">
      {/* 11px 头 */}
      <div className="mb-0.5 flex items-center gap-2 text-[11px] text-fg-3">
        <span
          data-testid="assistant-avatar"
          className="flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-md bg-fg font-serif text-[11px] text-fg-inv"
        >
          铸
        </span>
        <span>Agent</span>
        <StatusDot color={dotColor} pulse={streaming} />
        {msg.model && <span data-testid="assistant-model">{msg.model}</span>}
        <span className="flex-1" />
        {msg.time !== '' && <span>{msg.time}</span>}
      </div>
      {/* 时间线 */}
      {timeline.map((item, i) => {
        if (item.type === 'activity') {
          return <ActivitySegment key={i} blocks={blocks} indices={item.indices} />;
        }
        const block = blocks[item.index];
        const isTrailing = item.index === blocks.length - 1;
        switch (block.kind) {
          case 'text':
            if (item.index === finalTextIdx) {
              // 最终回答:完整 markdown,永不折叠
              return (
                <MarkdownFlat key={i} text={block.text} streaming={streaming && isTrailing} />
              );
            }
            // 中间叙述:text_2 13px 内联
            return (
              <div key={i} className="text-[13px] text-fg-2">
                <MarkdownFlat text={block.text} streaming={false} />
              </div>
            );
          case 'reasoning':
            return <ReasoningLine key={i} block={block} live={streaming && isTrailing} />;
          case 'subagent':
            return <SubagentRow key={i} block={block} />;
          case 'tool':
            // write_todos 里程碑(参考;本仓无该工具,组件就绪)
            return <TodoMilestoneLine key={i} block={block} />;
          default:
            return null;
        }
      })}
      {streaming && <StreamCaret />}
      {msg.status === 'failed' && msg.error && (
        <div className="text-[12px] text-danger" data-testid="assistant-error">
          {msg.error}
        </div>
      )}
    </div>
  );
}

/** 思考折叠行(参考:「思考 · {摘录} · {N} 字」,点击展开 12px text_4 全文)。 */
function ReasoningLine({
  block,
  live,
}: {
  block: Extract<ChatBlock, { kind: 'reasoning' }>;
  live: boolean;
}) {
  const [open, setOpen] = useState(false);
  return (
    <SummaryLine
      text={`思考 · ${reasoningSummary(block.text)}`}
      running={live}
      expanded={open}
      onToggle={() => setOpen((v) => !v)}
      testId="reasoning-line"
    >
      <div className="whitespace-pre-wrap text-[12px] text-fg-4">{block.text}</div>
    </SummaryLine>
  );
}

/** write_todos 里程碑行(参考:「待办 · {headline}」;本仓无该工具,组件就绪)。 */
function TodoMilestoneLine({ block }: { block: Extract<ChatBlock, { kind: 'tool' }> }) {
  const [open, setOpen] = useState(false);
  return (
    <SummaryLine
      text={`待办 · ${todoMilestoneLabel(block.args)}`}
      expanded={open}
      onToggle={() => setOpen((v) => !v)}
      testId="todo-milestone"
    >
      <div className="font-code text-[11.5px] whitespace-pre-wrap text-fg-4">{block.args}</div>
    </SummaryLine>
  );
}
