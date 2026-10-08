import { Loader2, Palette, Play, RotateCcw } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import {
  DESIGN_STAGES,
  designPhaseText,
  designStageIndex,
  designStageLabel,
  useDesignFlowStore,
} from '@/lib/designFlowStore';

/**
 * D-045:Design 流程状态条(Composer 上方):三步进度 + 当前状态 + 唯一的上下文动作。
 * 复刻轮失败 / 中断时给「继续复刻」;任何非运行态都可以「重新开始」(清掉流程指针,产物保留)。
 * 审阅、修改、修复的动作在聊天卡片上——这里不重复。
 */
export default function DesignBar() {
  const sessionId = useSessionStore((st) => st.activeSessionId);
  const state = useDesignFlowStore((st) => st.state);
  const flowSession = useDesignFlowStore((st) => st.sessionId);
  const pending = useDesignFlowStore((st) => st.pending);
  const activeRunId = useChatStore((st) => st.activeRunId);
  const flow = state !== null && flowSession === sessionId ? state : null;
  if (!flow || (flow.stage === 'done' && flow.phase !== 'running')) return null;

  const busy = flow.phase === 'running' || activeRunId !== null || pending !== null;
  const canResume = flow.stage === 'replication' && flow.phase !== 'running';
  const steps = DESIGN_STAGES.filter((s) => s !== 'done');
  const at = designStageIndex(flow.stage);

  return (
    <div
      data-testid="design-bar"
      data-stage={flow.stage}
      data-phase={flow.phase}
      className="flex min-w-0 items-center gap-2 rounded-lg border border-edge bg-shell-panel px-2.5 py-1.5 text-[11.5px]"
    >
      <Palette size={13} className="shrink-0 text-acc" />
      <span className="max-w-[30%] shrink-0 truncate font-medium text-fg-2" title={flow.title}>
        {flow.title || 'Design'}
      </span>
      <div className="flex shrink-0 items-center gap-1">
        {steps.map((s, i) => (
          <span
            key={s}
            title={designStageLabel(s)}
            className={cn('h-1.5 w-5 rounded-full', i < at ? 'bg-acc' : i === at ? 'bg-acc/60' : 'bg-edge')}
          />
        ))}
      </div>
      <span
        data-testid="design-bar-status"
        className={cn('min-w-0 flex-1 truncate', flow.phase === 'failed' ? 'text-warn' : 'text-fg-3')}
        title={designPhaseText(flow)}
      >
        {flow.phase === 'running' && <Loader2 size={11} className="mr-1 inline animate-spin" />}
        {designStageLabel(flow.stage)} · {designPhaseText(flow)}
      </span>
      {canResume && (
        <button
          type="button"
          data-testid="design-bar-resume"
          disabled={busy}
          onClick={() => void useDesignFlowStore.getState().act('resume_replication')}
          className="flex h-[22px] shrink-0 items-center gap-1 rounded-md border border-acc bg-acc px-2 text-[11px] text-fg-inv hover:bg-acc-soft disabled:cursor-not-allowed disabled:opacity-50"
        >
          <Play size={10} />
          继续复刻
        </button>
      )}
      <button
        type="button"
        data-testid="design-bar-restart"
        disabled={busy}
        title="结束这条流程(产物保留在 .forge/design/ 下),之后在 Design 模式里发消息开新流程"
        onClick={() => void useDesignFlowStore.getState().restart()}
        className="flex h-[22px] shrink-0 items-center gap-1 rounded-md px-1.5 text-[11px] text-fg-3 hover:bg-shell-hover hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-50"
      >
        <RotateCcw size={10} />
        重新开始
      </button>
    </div>
  );
}
