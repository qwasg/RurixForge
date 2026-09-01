import { Bot } from 'lucide-react';
import { useChatStore } from '@/lib/chatStore';
import {
  subagentDispatchSummary,
  subagentLiveSummary,
  type ChatBlock,
} from '@/lib/timeline';

/**
 * F7 wave.4 子代理极简行(参考 render_subagent_card):图标 + 双行摘要
 * (首行 12.5 text_2 派遣摘要;次行 11.5 text_4 进展/失败 danger);点击开 SubagentOverlay;
 * running 时 hover 出 Stop(= cancelRun)。本仓事件面不产生 subagent 块,组件就绪。
 */
export default function SubagentRow({
  block,
}: {
  block: Extract<ChatBlock, { kind: 'subagent' }>;
}) {
  const openSubagent = useChatStore((st) => st.openSubagent);
  const cancelRun = useChatStore((st) => st.cancelRun);
  const top = subagentDispatchSummary(block.label, block.prompt ?? '');
  const bottom = subagentLiveSummary(block.summary, block.work, block.status);
  const running = block.status === 'running';

  return (
    <div
      data-testid="subagent-row"
      onClick={() => openSubagent(block.id)}
      className="group flex cursor-pointer items-start gap-2 py-1"
    >
      <span className="mt-0.5 flex h-[14px] w-[14px] shrink-0 items-center justify-center text-fg-3">
        <Bot size={12} />
      </span>
      <span className="flex min-w-0 flex-1 flex-col gap-px">
        <span className="text-[12.5px] text-fg-2">{top}</span>
        <span className={`text-[11.5px] ${block.status === 'error' ? 'text-danger' : 'text-fg-4'}`}>
          {bottom}
        </span>
      </span>
      {running && (
        <button
          type="button"
          data-testid="subagent-stop"
          onClick={(e) => {
            e.stopPropagation();
            void cancelRun();
          }}
          className="h-5 shrink-0 rounded-md border border-edge px-[7px] text-[11px] text-fg-3 opacity-0 transition-opacity hover:bg-shell-hover hover:text-fg-2 group-hover:opacity-100"
        >
          Stop
        </button>
      )}
    </div>
  );
}
