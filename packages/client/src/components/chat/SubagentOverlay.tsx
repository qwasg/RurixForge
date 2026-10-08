import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { ChevronDown, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import { HOME_COL_MAX, type ChatVariant } from '@/lib/chatVariant';
import {
  buildTimeline,
  segmentPhraseParts,
  segmentStats,
  subagentDispatchSummary,
  type BlockStatus,
  type ChatBlock,
  type TimelineItem,
} from '@/lib/timeline';
import ActivitySegment, { SummaryLine, ToolLine } from './ActivitySegment';
import MarkdownFlat from './MarkdownFlat';
import StreamEnter, { useFreshKeys } from './StreamEnter';
import SubagentParticles from './SubagentParticles';
import { useCollaborationStore } from '@/lib/collaborationStore';
import AgentMailbox from './AgentMailbox';
import ApprovalCard from './ApprovalCard';
import SubagentRow from './SubagentRow';

type SubagentBlock = Extract<ChatBlock, { kind: 'subagent' }>;

/**
 * 状态徽标:文字 + 语义底色(与 SubagentRow 的粒子色一致:acc / sage / 中性)。
 * 留痕(2026-09-03 用户指令「报错不需特别标明」):失败档退出 danger 红,只留中性灰底,
 * 文字仍如实写「失败」——去的是颜色喊话,不是事实。
 */
const STATUS_BADGE: Record<BlockStatus, { text: string; cls: string }> = {
  running: { text: '运行中', cls: 'bg-acc-bg text-acc' },
  done: { text: '已完成', cls: 'bg-sage-bg text-sage' },
  error: { text: '失败', cls: 'bg-shell-sunk text-fg-3' },
};

function findSubagent(messages: ChatMsg[], id: string): SubagentBlock | null {
  const walk = (blocks: ChatBlock[]): SubagentBlock | null => {
    for (const block of blocks) {
      if (block.kind !== 'subagent') continue;
      if (block.id === id || block.agentId === id) return block;
      const nested = walk(block.work);
      if (nested) return nested;
    }
    return null;
  };
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const found = walk(messages[i].blocks);
    if (found) return found;
  }
  return null;
}

/**
 * 子代理浮层:头(状态粒子 + 标题 + 状态徽标 + 关闭)/ PROMPT(默认折叠 3 行)/
 * SUMMARY / WORK(嵌套时间线),三区以细分隔线分段。
 * home 变体跟着全屏主页走:收进居中列宽,并抬高到大输入盒之上。
 * Esc 关闭(子代理浮层不归 overlayStore 管,这里自理,语义对齐 Shell 的「Esc 关全部浮层」);
 * 切换到另一个子代理时以 id 为 key 重挂,入场动效重放、各区展开态归零。
 */
export default function SubagentOverlay({ variant = 'column' }: { variant?: ChatVariant }) {
  const overlayId = useChatStore((st) => st.subagentOverlayId);
  const messages = useChatStore((st) => st.messages);
  const openSubagent = useChatStore((st) => st.openSubagent);
  const agents = useCollaborationStore((state) => state.agents);

  useEffect(() => {
    if (!overlayId) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') openSubagent(null);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [overlayId, openSubagent]);

  if (!overlayId) return null;
  const block = findSubagent(messages, overlayId);
  const agent = agents.find((participant) => participant.id === (block?.agentId ?? overlayId));
  const found: SubagentBlock | null = block ?? (agent ? {
    kind: 'subagent', id: agent.id, agentId: agent.id, label: agent.name,
    status: agent.status === 'running' ? 'running' : agent.status === 'recoveryRequired' ? 'error' : 'done', work: [],
  } : null);
  if (!found) return null;

  const badge = STATUS_BADGE[found.status];
  const running = found.status === 'running';
  const title = subagentDispatchSummary(found.label, found.prompt ?? '');
  const prompt = (found.prompt ?? '').trim();
  const summary = (found.summary ?? '').trim();
  const work = found.work;
  const turns = agent ? messages.filter((message) => message.role === 'assistant' && message.agentId === agent.id) : [];
  const home = variant === 'home';

  return (
    <div
      key={agent?.id ?? found.id}
      role="dialog"
      aria-label={`子代理:${title}`}
      data-testid="subagent-overlay"
      style={home ? { maxWidth: HOME_COL_MAX - 28 } : undefined}
      className={cn(
        'forge-pop-in absolute z-30 flex max-h-[360px] flex-col overflow-hidden rounded-[10px] border border-edge bg-shell-float shadow-float',
        home
          ? 'inset-x-0 bottom-[184px] mx-auto w-[calc(100%-56px)]'
          : 'inset-x-[14px] bottom-[118px]',
      )}
    >
      <div className="flex shrink-0 items-center gap-2 border-b border-edge py-2 pl-2.5 pr-2">
        <span className="flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-md bg-shell-sunk">
          <SubagentParticles status={found.status} size={16} />
        </span>
        <span
          title={found.label}
          className="min-w-0 flex-1 truncate text-[12.5px] font-semibold text-fg"
        >
          {title}
        </span>
        <span
          data-testid="subagent-overlay-status"
          className={cn(
            'flex shrink-0 items-center gap-1.5 rounded-full px-2 py-[2px] text-[10.5px] font-medium',
            badge.cls,
          )}
        >
          {running && (
            <span className="h-[5px] w-[5px] shrink-0 animate-pulse rounded-full bg-current" />
          )}
          {agent?.status === 'idle' ? '空闲' : agent?.status === 'recoveryRequired' ? '需要恢复' : agent?.status === 'stopped' ? '已停止' : badge.text}
        </span>
        <button
          type="button"
          aria-label="关闭子代理详情"
          data-testid="subagent-overlay-close"
          onClick={() => openSubagent(null)}
          className="flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg"
        >
          <X size={13} />
        </button>
      </div>

      <div className="flex min-h-0 flex-col divide-y divide-edge overflow-y-auto px-3">
        {prompt !== '' && <PromptSection text={prompt} />}

        <Section label="SUMMARY">
          {summary !== '' ? (
            <MarkdownFlat text={summary} />
          ) : (
            <span className="flex items-center gap-1.5 text-[12px] text-fg-4">
              {running && (
                <span className="h-[6px] w-[6px] shrink-0 animate-pulse rounded-full bg-dot-running" />
              )}
              {running ? '子 agent 正在工作，完成后会在这里显示摘要。' : '无摘要输出。'}
            </span>
          )}
        </Section>

        {work.length > 0 && (
          <Section label="WORK" testId="subagent-work">
            <WorkTimeline work={work} streaming={running} />
          </Section>
        )}
        {turns.map((turn) => <Section key={turn.id} label={`${turn.time} · ${turn.status === 'streaming' ? '运行中' : '执行记录'}`}>
          <WorkTimeline work={turn.blocks} streaming={turn.status === 'streaming'} />
        </Section>)}
        {agent && <Section label="MESSAGES" testId="subagent-messages"><AgentMailbox key={agent.id} agent={agent} /></Section>}
      </div>
    </div>
  );
}

/** 分区:10px 加宽字距眉标(可带右侧操作)+ 内容;相邻分区由父级 divide-y 画细线。 */
function Section({
  label,
  aside,
  testId,
  children,
}: {
  label: string;
  aside?: ReactNode;
  testId?: string;
  children: ReactNode;
}) {
  return (
    <section data-testid={testId} className="flex flex-col gap-1.5 py-2.5">
      <div className="flex h-4 items-center justify-between gap-2">
        <span className="text-[10px] font-semibold tracking-wide text-fg-4">{label}</span>
        {aside}
      </div>
      {children}
    </section>
  );
}

/**
 * PROMPT 区:沉底引用块,默认夹到 3 行;只有真溢出(按实际排版测量)才出「展开」钮,
 * 短提示词不打扰。点眉标钮或点块体都可切换。
 */
function PromptSection({ text }: { text: string }) {
  const [expanded, setExpanded] = useState(false);
  const [overflowing, setOverflowing] = useState(false);
  const clampRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const el = clampRef.current;
    if (!el || expanded) return;
    const measure = () => setOverflowing(el.scrollHeight > el.clientHeight + 1);
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [text, expanded]);

  const toggleable = overflowing || expanded;
  const toggle = () => setExpanded((v) => !v);

  return (
    <Section
      label="PROMPT"
      aside={
        toggleable ? (
          <button
            type="button"
            data-testid="subagent-prompt-toggle"
            aria-expanded={expanded}
            onClick={toggle}
            className="flex items-center gap-0.5 rounded px-1 text-[10.5px] text-fg-4 transition-colors hover:text-fg-2"
          >
            {expanded ? '收起' : '展开'}
            <ChevronDown
              size={11}
              className={cn('transition-transform duration-150', expanded && 'rotate-180')}
            />
          </button>
        ) : null
      }
    >
      <div
        onClick={toggleable ? toggle : undefined}
        className={cn(
          'rounded-md bg-shell-sunk px-2.5 py-2',
          toggleable && 'cursor-pointer transition-colors hover:bg-shell-active',
        )}
      >
        <div
          ref={clampRef}
          className={cn(
            'whitespace-pre-wrap break-words text-[12px] leading-[1.55] text-fg-2',
            !expanded && 'line-clamp-3',
          )}
        >
          {text}
        </div>
      </div>
    </Section>
  );
}

/**
 * WORK 时间线:末尾黑色 text 块(子代理汇报输出)出现时,其上方的灰色过程行
 * (tool/reasoning)自动折叠为一行统计(如「Explored 6 files, 5 searches」),点击展开回看;
 * 无汇报输出(运行中)或汇报前无过程时原样逐行渲染。
 * D-047:streaming(子代理 / 该轮仍在跑)时运行中的行扫光;浮层打开之后才到的项自上而下入场
 * (打开那一刻已在的项随浮层 forge-pop-in 一起出现,不再各播一遍)。
 */
function WorkTimeline({ work, streaming = false }: { work: ChatBlock[]; streaming?: boolean }) {
  const [showProc, setShowProc] = useState(false);
  const timeline = buildTimeline(work);
  const itemKey = (item: TimelineItem) => (item.type === 'activity' ? item.indices[0] : item.index);
  const isFresh = useFreshKeys(timeline.map(itemKey));

  const renderBody = (item: TimelineItem): ReactNode => {
    if (item.type === 'activity') {
      return <ActivitySegment blocks={work} indices={item.indices} streaming={streaming} />;
    }
    const block = work[item.index];
    if (block.kind === 'tool') return <ToolLine block={block} streaming={streaming} />;
    if (block.kind === 'text') return <MarkdownFlat text={block.text} />;
    if (block.kind === 'approval') return <ApprovalCard block={block} />;
    if (block.kind === 'subagent') return <SubagentRow block={block} />;
    if (block.kind === 'reasoning') {
      return <div className="whitespace-pre-wrap text-[12px] text-fg-4">{block.text}</div>;
    }
    return null;
  };
  const renderItem = (item: TimelineItem, key: number) => {
    const body = renderBody(item);
    if (body === null) return null;
    return (
      <StreamEnter key={key} active={streaming && isFresh(itemKey(item))}>
        {body}
      </StreamEnter>
    );
  };

  // 最后一个 text 块(黑色汇报)在 timeline 中的位置
  let lastTextPos = -1;
  timeline.forEach((item, pos) => {
    if (item.type === 'block' && work[item.index].kind === 'text') lastTextPos = pos;
  });
  if (lastTextPos <= 0) return <>{timeline.map((item, i) => renderItem(item, i))}</>;

  const procItems = timeline.slice(0, lastTextPos);
  // 统计前置过程里的工具调用(files/searches 等;reasoning 不计数,仅随过程一起折叠)
  const toolIndices = procItems.flatMap((item) =>
    item.type === 'activity'
      ? item.indices
      : work[item.index].kind === 'tool'
        ? [item.index]
        : [],
  );
  const parts = segmentPhraseParts(segmentStats(work, toolIndices));
  // 全为 reasoning 时统计为空(段短语退到 Working),兜底报过程条数
  const steps = procItems.length;
  const head =
    parts.verb === 'Working'
      ? { verb: 'Traced', detail: `${steps} step${steps === 1 ? '' : 's'}` }
      : parts;

  return (
    <>
      <SummaryLine
        verb={head.verb}
        detail={head.detail}
        chevron
        expanded={showProc}
        onToggle={() => setShowProc((v) => !v)}
        testId="subagent-proc-toggle"
      >
        <div className="flex flex-col gap-1">
          {procItems.map((item, i) => renderItem(item, i))}
        </div>
      </SummaryLine>
      {timeline.slice(lastTextPos).map((item, i) => renderItem(item, lastTextPos + i))}
    </>
  );
}
