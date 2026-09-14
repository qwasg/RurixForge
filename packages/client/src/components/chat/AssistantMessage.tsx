import { useState } from 'react';
import { type ChatMsg } from '@/lib/chatStore';
import { buildTimeline, todoMilestoneLabel, type ChatBlock } from '@/lib/timeline';
import MarkdownFlat from './MarkdownFlat';
import ActivitySegment, { ReasoningLine, SummaryLine } from './ActivitySegment';
import SubagentRow from './SubagentRow';
import StreamCaret from './StreamCaret';
import ApprovalCard from './ApprovalCard';
import PlanBlock from './PlanBlock';
import { StatusDot } from '../shell/primitives';

/**
 * F7 wave.4 助手消息(参考 render_agent_message):
 * 头 = 22×22 圆角 6「铸」方块(bg=text text_inv serif 11px,替参考「月」)+「Agent」
 * + 状态点(streaming 脉冲/completed sage/其余灰)+ 模型 label + 右侧时间。
 * blocks 经 buildTimeline:text 块 = 中文正文,MarkdownFlat 加黑加粗全量不折叠;
 * tool/reasoning 段 = ActivitySegment(英文过程链两段式灰行,思考行并列其中);
 * write_todos 里程碑 =「Todos · …」折叠行;subagent = SubagentRow。streaming 时消息末挂 StreamCaret。
 *
 * 2026-09-03 用户指令留痕:
 * ① 正文强化 —— 中间叙述与最终回答同款(不再把中间叙述压成 13px text_2 弱文),
 *    统一交 MarkdownFlat strong 渲染(text 全黑 + font-bold),与灰色过程链拉开层级;
 *    流式期间也不再退成 text_2 灰,免得「正在说的话」比说完的话淡。
 * ② 报错不特别标明 —— 失败态状态点退成 dot-idle、错误行退成 text_3 灰(原 danger 红下线),
 *    错误原文照旧如实上屏,只是不再用颜色喊话。
 * ③ 思考行不再由本组件单列渲染(reasoning 已并进活动段);此处仅留防御分支,
 *    应对直接构造的 reasoning 块(旧快照/测试)。
 */
export default function AssistantMessage({ msg }: { msg: ChatMsg }) {
  const streaming = msg.status === 'streaming';
  const dotColor =
    msg.status === 'completed'
      ? 'var(--dot-done)'
      : msg.status === 'streaming'
        ? 'var(--dot-running)'
        : 'var(--dot-idle)';

  const blocks = msg.blocks;
  const timeline = buildTimeline(blocks);
  const hasVisibleText = blocks.some(
    (b) => (b.kind === 'text' || b.kind === 'plan') && b.text.trim() !== '',
  );

  return (
    <div data-testid="assistant-message" className="flex flex-col gap-1">
      {/* 11px 头 */}
      <div className="mb-0.5 flex items-center gap-2 text-[11px] text-fg-3">
        <span
          data-testid="assistant-avatar"
          className="flex h-[22px] min-w-[22px] shrink-0 items-center justify-center rounded-md bg-fg px-1 font-serif text-[11px] text-fg-inv"
        >
          {msg.engine === 'codex' ? 'Codex' : '铸'}
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
          return (
            <ActivitySegment
              key={i}
              blocks={blocks}
              indices={item.indices}
              streaming={streaming}
            />
          );
        }
        const block = blocks[item.index];
        const isTrailing = item.index === blocks.length - 1;
        switch (block.kind) {
          case 'text':
            // 正文(中文):加黑加粗全量 markdown,永不折叠
            return (
              <MarkdownFlat key={i} text={block.text} strong streaming={streaming && isTrailing} />
            );
          case 'reasoning':
            return <ReasoningLine key={i} block={block} live={streaming && isTrailing} />;
          case 'subagent':
            return <SubagentRow key={i} block={block} />;
          case 'tool':
            // write_todos 里程碑(参考;本仓无该工具,组件就绪)
            return <TodoMilestoneLine key={i} block={block} />;
          case 'plan':
            return <PlanBlock key={i} block={block} />;
          case 'approval':
            return <ApprovalCard key={i} block={block} />;
          default:
            return null;
        }
      })}
      {streaming && <StreamCaret />}
      {msg.status === 'completed' && !hasVisibleText && !msg.error && (
        <div className="text-[12px] text-fg-3" data-testid="assistant-empty">
          模型没有返回正文
        </div>
      )}
      {msg.status === 'failed' && msg.error && (
        <div className="text-[12px] text-fg-3" data-testid="assistant-error">
          {msg.error}
        </div>
      )}
    </div>
  );
}

/** write_todos 里程碑行(参考:「Todos · {headline}」;本仓无该工具,组件就绪)。 */
function TodoMilestoneLine({ block }: { block: Extract<ChatBlock, { kind: 'tool' }> }) {
  const [open, setOpen] = useState(false);
  return (
    <SummaryLine
      verb="Todos"
      detail={todoMilestoneLabel(block.args)}
      chevron
      expanded={open}
      onToggle={() => setOpen((v) => !v)}
      testId="todo-milestone"
    >
      <div className="font-code text-[11.5px] whitespace-pre-wrap text-fg-4">{block.args}</div>
    </SummaryLine>
  );
}
