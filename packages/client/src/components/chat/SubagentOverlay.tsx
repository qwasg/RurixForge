import { GitFork, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useChatStore } from '@/lib/chatStore';
import { HOME_COL_MAX, type ChatVariant } from '@/lib/chatVariant';
import { buildTimeline, type ChatBlock } from '@/lib/timeline';
import ActivitySegment, { ToolLine } from './ActivitySegment';
import MarkdownFlat from './MarkdownFlat';

/**
 * 子代理浮层:PROMPT / SUMMARY / WORK(嵌套时间线)。
 * home 变体跟着全屏主页走:收进居中列宽,并抬高到大输入盒之上。
 */
export default function SubagentOverlay({ variant = 'column' }: { variant?: ChatVariant }) {
  const overlayId = useChatStore((st) => st.subagentOverlayId);
  const messages = useChatStore((st) => st.messages);
  const openSubagent = useChatStore((st) => st.openSubagent);

  if (!overlayId) return null;
  let found: Extract<ChatBlock, { kind: 'subagent' }> | null = null;
  for (let i = messages.length - 1; i >= 0 && !found; i -= 1) {
    for (const b of messages[i].blocks) {
      if (b.kind === 'subagent' && b.id === overlayId) {
        found = b;
        break;
      }
    }
  }
  if (!found) return null;
  const badge =
    found.status === 'running'
      ? { text: '运行中', cls: 'bg-acc-bg text-acc' }
      : found.status === 'done'
        ? { text: '已完成', cls: 'bg-shell-sunk text-sage' }
        : { text: '失败', cls: 'bg-shell-sunk text-danger' };
  const work = found.work;
  const timeline = buildTimeline(work);
  const home = variant === 'home';

  return (
    <div
      data-testid="subagent-overlay"
      style={home ? { maxWidth: HOME_COL_MAX - 28 } : undefined}
      className={cn(
        'absolute z-30 flex max-h-[360px] flex-col overflow-hidden rounded-[10px] border border-edge bg-shell-float shadow-float',
        home
          ? 'bottom-[184px] left-1/2 w-[calc(100%-56px)] -translate-x-1/2'
          : 'inset-x-[14px] bottom-[118px]',
      )}
    >
      <div className="flex items-center gap-2 border-b border-edge px-2.5 py-2">
        <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-acc-bg text-acc">
          <GitFork size={11} />
        </span>
        <span className="min-w-0 flex-1 truncate text-[12.5px] font-semibold text-fg">
          {found.label}
        </span>
        <span className={`rounded-[5px] px-2 py-[3px] text-[10.5px] ${badge.cls}`}>{badge.text}</span>
        <button
          type="button"
          aria-label="关闭子代理详情"
          data-testid="subagent-overlay-close"
          onClick={() => openSubagent(null)}
          className="flex h-[22px] w-[22px] items-center justify-center rounded-[5px] text-fg-3 hover:bg-shell-hover"
        >
          <X size={12} />
        </button>
      </div>
      <div className="flex max-h-[316px] flex-col gap-2.5 overflow-y-auto p-3">
        {found.prompt && found.prompt.trim() !== '' && (
          <div className="flex flex-col gap-1">
            <span className="text-[10px] font-semibold text-fg-4">PROMPT</span>
            <span className="whitespace-pre-wrap text-[12px] text-fg-2">{found.prompt}</span>
          </div>
        )}
        <div className="flex flex-col gap-1">
          <span className="text-[10px] font-semibold text-fg-4">SUMMARY</span>
          {found.summary && found.summary.trim() !== '' ? (
            <MarkdownFlat text={found.summary} />
          ) : (
            <span className="font-code text-[12px] text-fg-4">
              {found.status === 'running' ? '子 agent 正在工作，完成后会在这里显示摘要。' : '无摘要输出。'}
            </span>
          )}
        </div>
        {work.length > 0 && (
          <div className="flex flex-col gap-1" data-testid="subagent-work">
            <span className="text-[10px] font-semibold text-fg-4">WORK</span>
            {timeline.map((item, i) => {
              if (item.type === 'activity') {
                return <ActivitySegment key={i} blocks={work} indices={item.indices} />;
              }
              const block = work[item.index];
              if (block.kind === 'tool') return <ToolLine key={i} block={block} />;
              if (block.kind === 'text') return <MarkdownFlat key={i} text={block.text} />;
              if (block.kind === 'reasoning') {
                return (
                  <div key={i} className="whitespace-pre-wrap text-[12px] text-fg-4">
                    {block.text}
                  </div>
                );
              }
              return null;
            })}
          </div>
        )}
      </div>
    </div>
  );
}
