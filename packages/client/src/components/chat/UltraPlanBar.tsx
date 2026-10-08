import { Fragment, useEffect, useRef, useState } from 'react';
import { ChevronRight, Gamepad2, Info, Loader2, Play, RotateCcw, RotateCw, Wand2 } from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  failedTurnKind,
  phaseText,
  stageIndex,
  stageLabel,
  useUltraPlanStore,
  type ActResult,
  type UltraPlanStage,
  type UltraPlanState,
} from '@/lib/ultraPlanStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useFlowContext } from './ultraplan/flowContext';

/** 状态条上的六步(done 不占一步:流程走完状态条就收起)。 */
const STEPS: UltraPlanStage[] = [
  'discovery',
  'questionnaire',
  'demo_review',
  'plan_review',
  'production',
  'acceptance',
];

const RUNNING_TITLE = '有任务正在运行';
const PENDING_TITLE = '上一个操作还在提交中';

type BarActionId = 'retry-answer' | 'retry-start' | 'retry-resume' | 'resume' | 'open-demo';

interface BarAction {
  id: BarActionId;
  label: string;
}

/**
 * 当前关口下唯一的上下文动作(没有就不出按钮)。
 *
 * 失败后的「重试」只在「重发哪一个动作」没有歧义时才给:
 * - 问卷关口:答案已落库(answers.json 存在)或失败码指明断在需求 / Demo 构建 → 不带答案重发
 *   answer(后端复用已存答案)。断在重新生成问卷(discovery)的不给——那要用户重新输入。
 * - 计划关口:只有确知断的是制作起步才重发 start_production。断在「改计划」的那一轮需要
 *   修改意见原文,这里拿不到;更不能把一次失败的改计划「重试」成开始制作。
 * - 制作关口:resume_production。
 * Demo 关口(等待 / 失败):「打开 Demo」——只是开试玩页签,不是流程动作;通过 / 提出修改 /
 * 回到上一版都在页签与卡片上(失败后的重试也在那里:改意见要原文)。验收关口的动作在验收卡上。
 */
export function contextAction(flow: UltraPlanState, hasAnswers: boolean): BarAction | null {
  if (flow.stage === 'demo_review' && flow.phase !== 'running') {
    return { id: 'open-demo', label: '打开 Demo' };
  }
  if (flow.phase === 'failed') {
    const kind = failedTurnKind(flow);
    if (flow.stage === 'questionnaire') {
      return kind === 'spec_demo' || (kind === null && hasAnswers)
        ? { id: 'retry-answer', label: '重试' }
        : null;
    }
    if (flow.stage === 'plan_review') {
      return kind === 'production' ? { id: 'retry-start', label: '重试' } : null;
    }
    if (flow.stage === 'production') return { id: 'retry-resume', label: '重试' };
    return null;
  }
  if (flow.phase === 'waiting' && flow.stage === 'production') {
    return { id: 'resume', label: '继续制作' };
  }
  return null;
}

type Confirm = { kind: 'restart' } | { kind: 'ack'; action: BarAction };

/**
 * D-044:Composer 上方的 UltraPlan 状态条(GoalBar 同款位置与外形)。
 *
 * 一行:六步进度(需求 → 问卷 → Demo → 计划 → 制作 → 验收,当前步高亮)+ 一句阶段说明
 * (在跑什么 / 在等什么 / 断在哪;失败用 warn 色,原因在悬停提示里)+ 至多一个上下文动作
 * +「重新开始」(点开后就地二次确认)。对话列宽 300–560px:宽处是一行,窄处阶段说明与按钮
 * 折到第二行(图标与六步绑成一组,不会被裁掉)。
 *
 * 会话里有任务在跑(activeRunId 非空)时所有动作禁用;代理不是 coding 时
 * 整条只读并说明原因。只在当前会话有未完成的流程(或流程停在失败态)时出现。
 */
export default function UltraPlanBar() {
  const { activeSessionId, flow, activeRunId, unsupportedReason } = useFlowContext();
  const pending = useUltraPlanStore((st) => st.pending);
  const hasAnswers = useUltraPlanStore((st) => st.answers !== null);
  const live = useUltraPlanStore((st) => st.live);
  const notices = useUltraPlanStore((st) => st.notices);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const flowId = flow?.id ?? null;
  const stage = flow?.stage ?? null;
  const phase = flow?.phase ?? null;
  // 流程往前走了 / 换了流程:上一步挂着的二次确认作废。
  useEffect(() => {
    setConfirm(null);
  }, [flowId, stage, phase]);

  if (!flow || (flow.stage === 'done' && flow.phase !== 'failed')) return null;

  const at = stageIndex(flow.stage);
  const failed = flow.phase === 'failed';
  const running = flow.phase === 'running';
  const action = unsupportedReason === null ? contextAction(flow, hasAnswers) : null;
  // 「提交中」只认 store 的 pending(act / REST 动作发请求前同步置位,切会话时随 reset() 清掉)。
  // 状态条随 Composer 跨会话不重挂,组件里不另存在途标记——否则上一个会话没回来的请求会把它卡住。
  const blocked = activeRunId !== null ? RUNNING_TITLE : pending !== null ? PENDING_TITLE : null;
  const thinking =
    live !== null && (live.thinkingForced || live.effort !== null)
      ? ['深度思考', live.effort].filter((part): part is string => !!part).join(' · ')
      : null;
  const flowNotices = notices.filter((n) => n.id === '' || n.id === flow.id);

  // 「打开 Demo」只是开页签看,不发请求:有任务在跑 / 动作在途都不挡。
  const actionBlocked = action?.id === 'open-demo' ? null : blocked;

  const runAction = async (target: BarAction, acknowledge = false) => {
    if (target.id === 'open-demo') {
      if (activeSessionId) useWorkbenchStore.getState().openDemo(flow.id, activeSessionId, flow.title);
      return;
    }
    if (blocked !== null) return;
    const store = useUltraPlanStore.getState();
    const result: ActResult = await (target.id === 'retry-answer'
      ? store.submitAnswers()
      : target.id === 'retry-start'
        ? store.startProduction(acknowledge)
        : store.resumeProduction(acknowledge));
    if (!alive.current) return;
    // 逐项审批权限下制作需要一次明确确认(契约 §2 acknowledgeApprovals):就地问一句再带上重发。
    if (!result.ok && result.code === 'ULTRAPLAN_NEEDS_BYPASS' && !acknowledge) {
      setConfirm({ kind: 'ack', action: target });
    } else {
      setConfirm(null);
    }
  };

  const runRestart = async () => {
    if (blocked !== null) return;
    await useUltraPlanStore.getState().restart();
    if (alive.current) setConfirm(null);
  };

  const ghost =
    'flex h-[22px] shrink-0 items-center gap-1 rounded-md px-1.5 text-[10.5px] text-fg-3 hover:bg-shell-hover hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-50';

  return (
    <div
      data-testid="ultraplan-bar"
      data-stage={flow.stage}
      data-phase={flow.phase}
      className="overflow-hidden rounded-xl border border-edge bg-shell-panel"
    >
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 px-3 py-1.5">
        {/* 图标与六步绑成一组:列宽不够时整组折行(六步自身也可再折),不会只剩一个图标占一行,也不会被裁掉 */}
        <div className="flex min-w-0 max-w-full items-center gap-2">
          <span
            title={flow.title !== '' ? `UltraPlan · ${flow.title}` : 'UltraPlan'}
            className="flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-acc-bg text-acc"
          >
            <Wand2 size={11} />
          </span>
          <ol
            aria-label="UltraPlan 流程进度"
            data-testid="ultraplan-bar-steps"
            className="flex min-w-0 flex-wrap items-center text-[10px] leading-[16px]"
          >
            {STEPS.map((step, i) => {
              const state = i < at ? 'done' : i === at ? 'current' : 'todo';
              return (
                <Fragment key={step}>
                  {i > 0 && (
                    <li aria-hidden="true" className="-mx-px flex items-center text-fg-4">
                      <ChevronRight size={8} />
                    </li>
                  )}
                  <li
                    data-testid={`ultraplan-step-${step}`}
                    data-state={state}
                    aria-current={state === 'current' ? 'step' : undefined}
                    className={cn(
                      'rounded px-1',
                      state === 'current'
                        ? failed
                          ? 'bg-warn-bg font-medium text-warn'
                          : 'bg-acc-bg font-medium text-acc'
                        : state === 'done'
                          ? 'text-fg-3'
                          : 'text-fg-4',
                    )}
                  >
                    {stageLabel(step)}
                  </li>
                </Fragment>
              );
            })}
          </ol>
        </div>
        <span
          data-testid="ultraplan-bar-phase"
          title={failed && flow.lastError?.message ? flow.lastError.message : undefined}
          className={cn(
            'flex min-w-0 flex-[1_1_120px] items-center gap-1 text-[10.5px]',
            failed ? 'text-warn' : 'text-fg-3',
          )}
        >
          {running && <Loader2 size={10} className="shrink-0 animate-spin" />}
          <span className="truncate">{failed ? `失败 · ${phaseText(flow)}` : phaseText(flow)}</span>
        </span>
        {thinking !== null && (
          <span
            data-testid="ultraplan-bar-thinking"
            className="shrink-0 rounded-full bg-shell-sunk px-1.5 py-px text-[9.5px] text-fg-3"
          >
            {thinking}
          </span>
        )}
        {flowNotices.length > 0 && (
          <span
            data-testid="ultraplan-bar-notice"
            title={flowNotices.map((n) => n.message || n.code).join('\n')}
            className="flex shrink-0 items-center gap-0.5 text-[9.5px] text-fg-4"
          >
            <Info size={10} />
            <span className="sr-only">{flowNotices.map((n) => n.message || n.code).join(';')}</span>
            提示 {flowNotices.length}
          </span>
        )}
        {unsupportedReason !== null ? (
          <span data-testid="ultraplan-bar-readonly" className="min-w-0 flex-[1_1_160px] text-[10.5px] text-fg-4">
            {unsupportedReason}
          </span>
        ) : (
          <div className="ml-auto flex shrink-0 items-center gap-1">
            {action && (
              <button
                type="button"
                data-testid="ultraplan-bar-action"
                data-action={action.id}
                disabled={actionBlocked !== null}
                title={actionBlocked ?? undefined}
                onClick={() => void runAction(action)}
                className="flex h-[22px] shrink-0 items-center gap-1 rounded-md border border-acc bg-acc px-2 text-[10.5px] text-fg-inv hover:bg-acc-soft disabled:cursor-not-allowed disabled:opacity-50"
              >
                {action.id === 'resume' ? (
                  <Play size={10} />
                ) : action.id === 'open-demo' ? (
                  <Gamepad2 size={10} />
                ) : (
                  <RotateCw size={10} />
                )}
                {action.label}
              </button>
            )}
            <button
              type="button"
              data-testid="ultraplan-bar-restart"
              aria-expanded={confirm?.kind === 'restart'}
              disabled={blocked !== null}
              title={blocked ?? '清除当前流程进度,从头再来'}
              onClick={() => setConfirm((cur) => (cur?.kind === 'restart' ? null : { kind: 'restart' }))}
              className={ghost}
            >
              <RotateCcw size={10} />
              重新开始
            </button>
          </div>
        )}
      </div>
      {confirm !== null && unsupportedReason === null && (
        <div
          role="group"
          aria-label={confirm.kind === 'restart' ? '确认重新开始' : '确认继续制作'}
          data-testid="ultraplan-bar-confirm"
          data-kind={confirm.kind}
          className="flex flex-wrap items-center gap-1.5 border-t border-edge px-3 py-1.5 text-[10.5px] leading-[15px] text-fg-3"
        >
          <span className="min-w-0 flex-[1_1_160px]">
            {confirm.kind === 'restart'
              ? '重新开始会清除当前流程进度(已生成的文件保留)。'
              : '当前权限下制作期间的每次写入都要你逐项确认(60 秒超时),无人值守会中断。仍要继续?'}
          </span>
          <button
            type="button"
            data-testid={confirm.kind === 'restart' ? 'ultraplan-bar-restart-cancel' : 'ultraplan-bar-ack-cancel'}
            onClick={() => setConfirm(null)}
            className={ghost}
          >
            取消
          </button>
          <button
            type="button"
            data-testid={confirm.kind === 'restart' ? 'ultraplan-bar-restart-confirm' : 'ultraplan-bar-ack-confirm'}
            disabled={blocked !== null}
            title={blocked ?? undefined}
            onClick={() =>
              void (confirm.kind === 'restart' ? runRestart() : runAction(confirm.action, true))
            }
            className="flex h-[22px] shrink-0 items-center rounded-md border border-warn/30 bg-warn-bg px-2 text-[10.5px] text-warn hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {confirm.kind === 'restart' ? '确认重新开始' : '仍然继续'}
          </button>
        </div>
      )}
    </div>
  );
}
