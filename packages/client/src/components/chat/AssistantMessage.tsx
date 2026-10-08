import { useState, type ReactNode } from 'react';
import { Copy } from 'lucide-react';
import { type ChatMsg } from '@/lib/chatStore';
import { copyText } from '@/lib/clipboard';
import { buildTimeline, todoMilestoneLabel, type ChatBlock } from '@/lib/timeline';
import MarkdownFlat from './MarkdownFlat';
import ActivitySegment, { ReasoningLine, SummaryLine } from './ActivitySegment';
import SubagentRow from './SubagentRow';
import StreamEnter from './StreamEnter';
import TurnStatusLine from './TurnStatusLine';
import ApprovalCard from './ApprovalCard';
import PlanBlock from './PlanBlock';
import UltraPlanBlock from './ultraplan/UltraPlanBlock';
import DesignBlock from './design/DesignBlock';
import GeneratedImage from './GeneratedImage';
import CloudErrorGuidance, { cloudGuidance } from './CloudErrorGuidance';
import { StatusDot } from '../shell/primitives';
import ForgeLogo from '@/components/ForgeLogo';

/**
 * F7 wave.4 助手消息(参考 render_agent_message):
 * 头 = 22×22 项目 logo(Codex 引擎显示 Codex 标记)+「Agent」
 * + 状态点(streaming 脉冲/completed sage/其余灰)+ 模型 label + 右侧时间。
 * blocks 经 buildTimeline:text 块 = 中文正文,MarkdownFlat 加黑加粗全量不折叠;
 * tool/reasoning 段 = ActivitySegment(英文过程链两段式灰行,思考行并列其中);
 * write_todos 里程碑 =「Todos · …」折叠行;subagent = SubagentRow。
 *
 * D-047(Cursor 式等待显示):streaming 时消息末挂 TurnStatusLine(Planning next moves /
 * Taking longer than expected / Reconnecting / Connection lost / Waiting for approval,
 * 状态机见 lib/turnStatus.ts),取代原 accent 闪烁 caret;流式中新出现的时间线项经 StreamEnter
 * 自上而下入场,正文则按 markdown 块逐块入场(MarkdownFlat animate),外层不再叠一层。
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

  const blocks = msg.blocks;
  const timeline = buildTimeline(blocks);
  // D-044:以 UltraPlan 关口卡收尾的轮没有正文,卡片本身就是可见产出,不算「没有返回正文」。
  const hasVisibleText = blocks.some(
    (b) =>
      b.kind === 'ultraplan' || b.kind === 'image' || ((b.kind === 'text' || b.kind === 'plan') && b.text.trim() !== ''),
  );
  /** 复制 = 助手对用户说的正文(过程链/工具输出不在内),按块空行拼接。 */
  const replyText = blocks
    .filter((b): b is Extract<ChatBlock, { kind: 'text' }> => b.kind === 'text')
    .map((b) => b.text.trim())
    .filter((t) => t !== '')
    .join('\n\n');

  return (
    <div data-testid="assistant-message" className="group/msg flex flex-col gap-1">
      <AssistantHeader engine={msg.engine} status={msg.status} model={msg.model} time={msg.time} />
      {/* 时间线 */}
      {timeline.map((item, i) => {
        if (item.type === 'activity') {
          return (
            <StreamEnter key={i} active={streaming}>
              <ActivitySegment blocks={blocks} indices={item.indices} streaming={streaming} />
            </StreamEnter>
          );
        }
        const block = blocks[item.index];
        const isTrailing = item.index === blocks.length - 1;
        if (block.kind === 'text') {
          // 正文(中文):加黑加粗全量 markdown,永不折叠;入场交给 MarkdownFlat 逐块播
          return (
            <StreamEnter key={i} active={false}>
              <MarkdownFlat text={block.text} strong streaming={streaming && isTrailing} animate={streaming} />
            </StreamEnter>
          );
        }
        return (
          <StreamEnter key={i} active={streaming}>
            {renderBlock(block, streaming && isTrailing)}
          </StreamEnter>
        );
      })}
      {streaming && <TurnStatusLine blocks={blocks} />}
      {msg.status === 'completed' && !hasVisibleText && !msg.error && (
        <div className="text-[12px] text-fg-3" data-testid="assistant-empty">
          模型没有返回正文
        </div>
      )}
      {msg.status === 'failed' && cloudGuidance(msg.errorCode) ? (
        <CloudErrorGuidance code={msg.errorCode} error={msg.error} />
      ) : (
        msg.status === 'failed' &&
        msg.error && (
          <div className="text-[12px] text-fg-3" data-testid="assistant-error">
            {msg.error}
          </div>
        )
      )}
      {!streaming && replyText !== '' && (
        <div className="flex h-6 items-center opacity-0 transition-opacity focus-within:opacity-100 group-hover/msg:opacity-100">
          <button
            type="button"
            data-testid="assistant-copy"
            aria-label="复制回复"
            title="复制回复"
            onClick={() => void copyText(replyText, '回复')}
            className="flex h-6 items-center gap-1 rounded-md px-1.5 text-[11px] text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2"
          >
            <Copy size={12} />
            复制
          </button>
        </div>
      )}
    </div>
  );
}

/** 非正文时间线块(text 由调用方单列,入场方式不同)。 */
function renderBlock(block: ChatBlock, live: boolean): ReactNode {
  switch (block.kind) {
    case 'image':
      return <GeneratedImage block={block} />;
    case 'reasoning':
      return <ReasoningLine block={block} live={live} />;
    case 'subagent':
      return <SubagentRow block={block} />;
    case 'tool':
      // write_todos 里程碑(参考;本仓无该工具,组件就绪)
      return <TodoMilestoneLine block={block} />;
    case 'plan':
      return <PlanBlock block={block} />;
    case 'approval':
      return <ApprovalCard block={block} />;
    case 'ultraplan':
      return <UltraPlanBlock block={block} />;
    case 'design':
      return <DesignBlock block={block} />;
    default:
      return null;
  }
}

/**
 * 11px 头:22×22 项目 logo(Codex 引擎显示 Codex 标记)+「Agent」+ 状态点
 * (streaming 脉冲 / completed sage / 其余灰)+ 模型 label + 右侧时间。真卡与开轮前占位卡共用。
 */
function AssistantHeader({
  engine,
  status,
  model,
  time = '',
}: {
  engine?: ChatMsg['engine'];
  status?: ChatMsg['status'];
  model?: string;
  time?: string;
}) {
  const dotColor =
    status === 'completed' ? 'var(--dot-done)' : status === 'streaming' ? 'var(--dot-running)' : 'var(--dot-idle)';
  return (
    <div className="mb-0.5 flex items-center gap-2 text-[11px] text-fg-3">
      {engine === 'codex' ? (
        <span
          data-testid="assistant-avatar"
          className="flex h-[22px] min-w-[22px] shrink-0 items-center justify-center rounded-md bg-fg px-1 font-serif text-[11px] text-fg-inv"
        >
          Codex
        </span>
      ) : (
        <ForgeLogo data-testid="assistant-avatar" className="h-[22px] w-[22px] rounded-md" />
      )}
      <span>Agent</span>
      <StatusDot color={dotColor} pulse={status === 'streaming'} />
      {model && <span data-testid="assistant-model">{model}</span>}
      <span className="flex-1" />
      {time !== '' && <span>{time}</span>}
    </div>
  );
}

/** 开轮前占位卡的空时间线(身份恒定:状态行的空档计时只从发出时刻起算,不被重渲染刷新)。 */
const NO_BLOCKS: ChatBlock[] = [];

/**
 * D-047 开轮前占位卡:用户发出后、agent.started 建出真卡之前(Codex 冷启动可达数秒),
 * 用同款头 + 轮次状态行先接住(Planning next moves → Taking longer than expected / 断线态),
 * 计时从发出时刻起算;真卡出现即由 MessageList 原位换下,头部位置不跳。
 */
export function PendingAssistantMessage({ since, engine }: { since: number; engine?: ChatMsg['engine'] }) {
  return (
    <div data-testid="assistant-pending" className="flex flex-col gap-1">
      <AssistantHeader engine={engine} status="streaming" />
      <TurnStatusLine blocks={NO_BLOCKS} since={since} />
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
