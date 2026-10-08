import { cn } from '@/lib/cn';
import { useChatStore } from '@/lib/chatStore';
import {
  subagentDispatchSummary,
  subagentLiveSummary,
  type ChatBlock,
} from '@/lib/timeline';
import SubagentParticles from './SubagentParticles';
import { useCollaborationStore } from '@/lib/collaborationStore';

/**
 * F7 wave.4 子代理极简行(参考 render_subagent_card):状态粒子 + 双行摘要
 * (首行 12.5 text_2 派遣摘要;次行 11.5 text_4 进展/失败同色——2026-09-03 用户指令
 * 「报错不需特别标明」,原失败态 danger 红下线,文案仍如实);点击开 SubagentOverlay;
 * running 时 hover 出 Stop —— 后台子代理(D-036 detachedRunId)只中止它自己,
 * 同步子代理沿用中止父轮(cancelRun)。
 * 图标位由 SubagentParticles 承担:running 动态粒子 / done 静态粒子 / error 红粒子。
 */
export default function SubagentRow({
  block,
}: {
  block: Extract<ChatBlock, { kind: 'subagent' }>;
}) {
  const openSubagent = useChatStore((st) => st.openSubagent);
  const cancelRun = useChatStore((st) => st.cancelRun);
  const cancelSubagent = useChatStore((st) => st.cancelSubagent);
  const member = useCollaborationStore((state) => state.agents.find((agent) => agent.id === block.agentId));
  const top = subagentDispatchSummary(block.label, block.prompt ?? '');
  const bottom = subagentLiveSummary(block.summary, block.work, block.status);
  const running = block.status === 'running';
  // D-036:后台子代理有自己的 run,Stop 只打它一个;同步子代理没有,维持中止父轮原语义。
  const stop = () =>
    member?.activeRunId ? cancelSubagent(member.activeRunId) : block.agentId && block.agentRunId ? cancelSubagent(block.agentRunId) : block.detachedRunId ? cancelSubagent(block.detachedRunId) : cancelRun();

  return (
    <div
      data-testid="subagent-row"
      onClick={() => openSubagent(block.id)}
      className="group flex cursor-pointer items-start gap-2 py-1"
    >
      <span className="mt-0.5 flex h-[14px] w-[14px] shrink-0 items-center justify-center">
        <SubagentParticles status={block.status} />
      </span>
      <span className="flex min-w-0 flex-1 flex-col gap-px">
        <span className="text-[12.5px] text-fg-2">{top}</span>
        {/* D-047:运行中末行(实时进展)扫光;self-start 让行宽贴字,亮带不扫空白 */}
        <span className={cn('max-w-full self-start text-[11.5px]', running ? 'forge-shimmer' : 'text-fg-4')}>
          {bottom}
        </span>
      </span>
      {running && (
        <button
          type="button"
          data-testid="subagent-stop"
          onClick={(e) => {
            e.stopPropagation();
            void stop();
          }}
          className="h-5 shrink-0 rounded-md border border-edge px-[7px] text-[11px] text-fg-3 opacity-0 transition-opacity hover:bg-shell-hover hover:text-fg-2 group-hover:opacity-100"
        >
          Stop
        </button>
      )}
    </div>
  );
}
