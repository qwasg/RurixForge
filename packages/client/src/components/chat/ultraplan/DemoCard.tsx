import type { ReactNode } from 'react';
import { Gamepad2, Play } from 'lucide-react';
import type { ChatBlock } from '@/lib/timeline';
import {
  cardInteractive,
  stageIndex,
  stageLabel,
  useUltraPlanStore,
  type UltraPlanState,
} from '@/lib/ultraPlanStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import {
  DEMO_MANUAL_NOTE,
  DemoDecisionControls,
  ProbeErrors,
  VerifiedBadge,
  demoBlockReason,
  readProbe,
} from './DemoReview';
import { useFlowContext } from './flowContext';

type Ultra = Extract<ChatBlock, { kind: 'ultraplan' }>;

function str(payload: Record<string, unknown>, key: string): string {
  const value = payload[key];
  return typeof value === 'string' ? value : '';
}

function num(payload: Record<string, unknown>, key: string): number | null {
  const value = payload[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

/** 不是当前可操作的那一版:为什么(与问卷卡同一套措辞)。 */
function staleReason(block: Ultra, flow: UltraPlanState | null): string {
  if (!flow || block.upId !== flow.id) return '此流程属于其他会话,或已被重新开始';
  if (block.rev < flow.demoIteration) return `已有更新的 Demo(第 ${flow.demoIteration} 版)`;
  // 卡片比本地阶段新:建卡事件先到、阶段重拉还没回来(一瞬间)。
  if (block.rev > flow.demoIteration) return '正在同步流程状态…';
  if (flow.stage !== 'demo_review' && stageIndex(flow.stage) > stageIndex('demo_review')) {
    return `流程已进入「${stageLabel(flow.stage)}」阶段`;
  }
  return '正在同步流程状态…';
}

/**
 * D-044:Demo 卡(ultraplan.demo.ready 建出,一版一张)。
 *
 * 展示这一版的迭代号、自动验证徽标 + 说明、探测报错(可折叠)、探测截图的**路径**(截图是
 * 工作区文件,这里不拉取)、自动验证后仍需试玩确认体验的提醒;「打开 Demo」在工作台开
 * Demo 页签试玩。
 *
 * 三种形态:
 * - 可操作(契约 §9 cardInteractive):附「通过 / 提出修改」,与页签同一套控件;只被在跑的任务
 *   或不支持的引擎 / 代理临时挡住时,控件留着但禁用并说明原因。
 * - 已提交:block.submitted(ultraplan.demo.decision 回填)→「已通过」/「已要求修改:<意见>」。
 * - 只读说明:旧版 / 别的流程 / 关口已过,给一句原因。
 */
export default function DemoCard({ block }: { block: Ultra }) {
  const { activeSessionId, flow, activeRunId, unsupportedReason } = useFlowContext();
  const pending = useUltraPlanStore((st) => st.pending !== null);
  const openDemo = useWorkbenchStore((st) => st.openDemo);

  const payload = block.payload;
  const verified = payload.verified === true;
  const note = str(payload, 'note');
  const probe = readProbe(payload.probe);
  const rollbackOf = num(payload, 'rollbackOf');
  const ownFlow = flow !== null && block.upId === flow.id;

  // current = 这张卡就是当前关口的当前版(控件出现);interactive = 此刻还能点(契约 §9 + 引擎 /
  // 代理支持 + 没有在途动作)。current 但不能点的,控件禁用并说明原因。
  const current =
    ownFlow && flow.stage === 'demo_review' && block.rev === flow.demoIteration && !block.submitted;
  const interactive = cardInteractive(block, flow, activeRunId) && unsupportedReason === null && !pending;
  const blocked = interactive
    ? null
    : (demoBlockReason({
        upId: block.upId,
        ownerSessionId: null,
        activeSessionId,
        flow,
        activeRunId,
        pending,
        unsupportedReason,
      }) ?? '正在同步流程状态…');

  let footer: ReactNode;
  if (block.submitted) {
    const decision = str(block.submitted, 'decision');
    const feedback = str(block.submitted, 'feedback').trim();
    footer = (
      <div data-testid="demo-card-submitted" data-decision={decision || undefined} className="text-[11px] text-fg-3">
        {decision === 'approve'
          ? '已通过'
          : decision === 'revise'
            ? `已要求修改${feedback !== '' ? `:${feedback}` : ''}`
            : '已处理'}
      </div>
    );
  } else if (current && flow) {
    footer = (
      <DemoDecisionControls
        key={`${block.upId}:${block.rev}`}
        upId={block.upId}
        iteration={block.rev}
        verified={verified}
        blocked={blocked}
        failed={flow.phase === 'failed'}
        testIdPrefix="demo-card"
        compact
      />
    );
  } else {
    footer = (
      <div data-testid="demo-card-readonly" className="text-[11px] text-fg-3">
        {staleReason(block, flow)}
      </div>
    );
  }

  return (
    <section
      data-testid="demo-card"
      data-interactive={current && interactive ? '1' : undefined}
      aria-label={`Demo 第 ${block.rev} 版`}
      className="my-1 overflow-hidden rounded-[10px] border border-edge-strong bg-shell-sunk shadow-sh1"
    >
      <div className="flex items-start gap-2.5 px-3 py-2.5">
        <span className="mt-px flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-acc-bg text-acc">
          <Gamepad2 size={13} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-[12.5px] font-medium leading-[18px] text-fg">Demo · 第 {block.rev} 版</span>
            <VerifiedBadge verified={verified} note={note || null} testId="demo-card-verified" />
          </div>
          {(rollbackOf !== null || probe?.unavailable) && (
            <div className="mt-0.5 text-[10.5px] leading-[15px] text-fg-4">
              {[
                rollbackOf !== null ? `由第 ${rollbackOf} 版回退` : '',
                probe?.unavailable ? '探测环境不可用' : '',
              ]
                .filter((part) => part !== '')
                .join(' · ')}
            </div>
          )}
        </div>
        {ownFlow && activeSessionId !== null && (
          <button
            type="button"
            data-testid="demo-card-open"
            title="在工作台打开 Demo 试玩"
            onClick={() => openDemo(block.upId, activeSessionId, flow.title)}
            className="flex h-[24px] shrink-0 items-center gap-1 rounded-md border border-edge bg-shell-panel px-2 text-[11px] text-fg-2 hover:bg-shell-hover"
          >
            <Play size={11} />
            打开 Demo
          </button>
        )}
      </div>
      <div className="flex flex-col gap-1.5 border-t border-edge px-3 py-2">
        {note !== '' && (
          <div data-testid="demo-card-note" className="break-words text-[11.5px] leading-[17px] text-fg-2">
            {note}
          </div>
        )}
        {probe && <ProbeErrors errors={probe.errors} testId="demo-card-probe-errors" />}
        {probe?.screenshot && (
          <div
            data-testid="demo-card-screenshot"
            className="break-all font-code text-[10.5px] leading-[15px] text-fg-3"
            title="探测截图(工作区文件)"
          >
            探测截图:{probe.screenshot}
          </div>
        )}
        <div className="text-[10.5px] leading-[15px] text-fg-4">{DEMO_MANUAL_NOTE}</div>
      </div>
      <div className="border-t border-edge px-3 py-2">{footer}</div>
    </section>
  );
}
