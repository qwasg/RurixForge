import { useEffect, useRef, useState } from 'react';
import { Check, ChevronDown, Loader2, MessageSquareText, TriangleAlert } from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  actionErrorText,
  phaseText,
  stageIndex,
  stageLabel,
  useUltraPlanStore,
  type UltraPlanState,
} from '@/lib/ultraPlanStore';

/**
 * D-044:Demo 页签(DemoTab)与 Demo 卡(DemoCard)共用的试玩评审件——
 * 「通过 / 提出修改」控件、自动验证徽标、探测报错折叠列表、动作禁用原因。
 *
 * 动作一律带「这一版」的 {id, rev}(流程 id + Demo 迭代号)经 ultraPlanStore 发出,
 * 过期的页签 / 卡片由后端 409 兜底;前端先按同一套条件禁用并说明原因。
 */

/** 自动验证会执行脚本化键鼠输入；手感仍由用户试玩判断。 */
export const DEMO_MANUAL_NOTE = '自动验证包含键鼠操作，请试玩确认手感与整体体验';
/** Workbench 只渲染激活的页签:切走即卸载 iframe,Demo 从头开始。 */
export const DEMO_RESET_NOTE = '切走页签会重置 Demo';
export const OTHER_SESSION_REASON = '该 Demo 属于另一个会话';
export const FLOW_GONE_REASON = '该 Demo 不属于当前流程(流程可能已重新开始)';
const RUNNING_REASON = '有任务正在运行';
const PENDING_REASON = '上一个操作还在提交中';

/** demo.ready 载荷里的 probe(字段一律按「可能缺失 / 类型不对」读)。 */
export interface ProbeInfo {
  ok: boolean;
  errors: string[];
  /** 探测环境不可用(没有 Node / Edge 等)。 */
  unavailable: boolean;
  /** 截图的工作区相对路径(不拉取,只显示路径)。 */
  screenshot: string | null;
}

export function readProbe(raw: unknown): ProbeInfo | null {
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) return null;
  const rec = raw as Record<string, unknown>;
  return {
    ok: rec.ok === true,
    errors: Array.isArray(rec.errors)
      ? rec.errors.filter((e): e is string => typeof e === 'string' && e.trim() !== '')
      : [],
    unavailable: rec.unavailable === true,
    screenshot: typeof rec.screenshot === 'string' && rec.screenshot !== '' ? rec.screenshot : null,
  };
}

export interface DemoGate {
  upId: string;
  /** 页签所属会话;null = 不区分(聊天里的卡片永远属于当前会话)。 */
  ownerSessionId: string | null;
  activeSessionId: string | null;
  /** 当前会话的流程(useFlowContext().flow);别的会话的页签传它自己取回的状态也一样被第一条挡住。 */
  flow: UltraPlanState | null;
  activeRunId: string | null;
  pending: boolean;
  unsupportedReason: string | null;
}

/**
 * 「通过 / 提出修改 / 回到上一版」为什么不能点(null = 可以)。顺序即优先级:
 * 别的会话 → 不是这个流程 → 引擎 / 代理不支持 → 关口不对 → 流程在跑 → 会话有 run → 动作在途。
 */
export function demoBlockReason(gate: DemoGate): string | null {
  if (gate.ownerSessionId !== null && gate.ownerSessionId !== gate.activeSessionId) {
    return OTHER_SESSION_REASON;
  }
  const flow = gate.flow;
  if (!flow || flow.id !== gate.upId) return FLOW_GONE_REASON;
  if (gate.unsupportedReason !== null) return gate.unsupportedReason;
  if (flow.stage !== 'demo_review') {
    return stageIndex(flow.stage) > stageIndex('demo_review')
      ? `流程已进入「${stageLabel(flow.stage)}」阶段`
      : '流程还没到 Demo 试玩阶段';
  }
  if (flow.phase === 'running') return `流程正在处理:${phaseText(flow)}`;
  if (gate.activeRunId !== null) return RUNNING_REASON;
  if (gate.pending) return PENDING_REASON;
  return null;
}

/** 已自动验证 / 未自动验证;说明(demoNote / 载荷 note)放悬停提示。未验证用 warn 色,不用 danger。 */
export function VerifiedBadge({
  verified,
  note,
  testId,
}: {
  verified: boolean;
  note: string | null;
  testId: string;
}) {
  const title =
    note && note.trim() !== '' ? note : verified ? '自动探测通过' : '自动探测没有确认这一版 Demo 可玩';
  return (
    <span
      data-testid={testId}
      data-verified={verified ? '1' : '0'}
      title={title}
      className={cn(
        'inline-flex shrink-0 items-center gap-0.5 rounded-full px-1.5 py-px text-[10px] leading-[15px]',
        verified ? 'bg-sage-bg text-sage' : 'bg-warn-bg text-warn',
      )}
    >
      {verified ? <Check size={10} /> : <TriangleAlert size={10} />}
      {verified ? '已自动验证' : '未自动验证'}
    </span>
  );
}

/** 探测报错(可折叠;没有报错不渲染)。 */
export function ProbeErrors({ errors, testId }: { errors: string[]; testId: string }) {
  const [open, setOpen] = useState(false);
  if (errors.length === 0) return null;
  return (
    <div data-testid={testId} className="min-w-0">
      <button
        type="button"
        aria-expanded={open}
        data-testid={`${testId}-toggle`}
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1 text-left text-[10.5px] leading-[15px] text-warn hover:opacity-90"
      >
        <ChevronDown size={10} className={cn('shrink-0 transition-transform', !open && '-rotate-90')} />
        探测报错 {errors.length} 条
      </button>
      {open && (
        <ul
          data-testid={`${testId}-list`}
          className="mt-1 flex max-h-40 flex-col gap-0.5 overflow-auto rounded-md border border-edge bg-shell-panel px-2 py-1.5"
        >
          {errors.map((e, i) => (
            <li key={i} className="break-words font-code text-[10.5px] leading-[15px] text-fg-2">
              {e}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * 「通过 Demo」+「提出修改」(文本框 + 提交)。未自动验证时通过钮读作「未自动验证,仍然通过」。
 * blocked 非 null = 全部禁用,原因写进每个按钮的 title 并在下方显示一行。
 * 提交被受理(或整轮已结束)后显示「已提交,等待处理…」;被拒的原因就地显示(warn 色),
 * store 同时弹 warning toast 并重拉真实阶段。流程上一轮失败(failed)时放开重发。
 */
export function DemoDecisionControls({
  upId,
  iteration,
  verified,
  blocked,
  failed,
  testIdPrefix,
  compact = false,
}: {
  upId: string;
  iteration: number;
  verified: boolean;
  blocked: string | null;
  failed: boolean;
  testIdPrefix: string;
  /** 聊天卡片里(300–560px 宽)用小一号的按钮。 */
  compact?: boolean;
}) {
  const [revising, setRevising] = useState(false);
  const [feedback, setFeedback] = useState('');
  const [submitting, setSubmitting] = useState<'approve' | 'revise' | null>(null);
  const [sent, setSent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    if (failed) setSent(false);
  }, [failed]);

  const busy = submitting !== null;
  const disabled = blocked !== null || busy || sent;
  const trimmed = feedback.trim();
  const p = testIdPrefix;
  const target = { id: upId, rev: iteration };

  const run = async (kind: 'approve' | 'revise') => {
    if (disabled) return;
    if (kind === 'revise' && trimmed === '') return;
    setSubmitting(kind);
    setError(null);
    const store = useUltraPlanStore.getState();
    const result =
      kind === 'approve' ? await store.approveDemo(target) : await store.reviseDemo(trimmed, target);
    if (!alive.current) return;
    setSubmitting(null);
    if (result.ok) {
      setSent(true);
      if (kind === 'revise') {
        setFeedback('');
        setRevising(false);
      }
      return;
    }
    setError(actionErrorText(result.code, result.message));
  };

  const btn = compact ? 'h-[24px] px-2 text-[11px]' : 'h-[26px] px-2.5 text-[12px]';
  const note = sent && blocked === null ? '已提交,等待处理…' : blocked;

  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <div className="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          data-testid={`${p}-approve`}
          data-verified={verified ? '1' : '0'}
          disabled={disabled}
          title={blocked ?? (verified ? '满意这一版,开始写制作计划' : '自动探测没有确认可玩;请先手动试玩再通过')}
          onClick={() => void run('approve')}
          className={cn(
            'flex items-center gap-1 rounded-md border border-acc bg-acc font-medium text-fg-inv hover:bg-acc-soft disabled:cursor-not-allowed disabled:opacity-50',
            btn,
          )}
        >
          {submitting === 'approve' ? <Loader2 size={11} className="animate-spin" /> : <Check size={11} />}
          {verified ? '通过 Demo' : '未自动验证,仍然通过'}
        </button>
        <button
          type="button"
          data-testid={`${p}-revise-toggle`}
          aria-expanded={revising}
          disabled={disabled}
          title={blocked ?? '写下想改的地方,按修改意见重做这一版'}
          onClick={() => setRevising((v) => !v)}
          className={cn(
            'flex items-center gap-1 rounded-md border border-edge bg-shell-panel text-fg-2 hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-50',
            btn,
          )}
        >
          <MessageSquareText size={11} />
          提出修改
        </button>
      </div>
      {revising && (
        <div className="flex min-w-0 flex-col gap-1.5">
          <textarea
            data-testid={`${p}-revise-input`}
            aria-label="修改意见"
            rows={3}
            value={feedback}
            disabled={disabled}
            placeholder="想改哪里?例如:敌人再慢一点、加一个暂停键…"
            onChange={(e) => {
              setFeedback(e.target.value);
              setError(null);
            }}
            className="w-full resize-y rounded-md border border-edge bg-shell-panel px-2 py-1.5 text-[12px] leading-[18px] text-fg outline-none placeholder:text-fg-4 focus:border-acc-ring disabled:cursor-not-allowed disabled:opacity-70"
          />
          <div className="flex items-center justify-end gap-1.5">
            <button
              type="button"
              data-testid={`${p}-revise-cancel`}
              disabled={busy}
              onClick={() => setRevising(false)}
              className={cn(
                'flex items-center rounded-md text-fg-3 hover:bg-shell-hover hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-50',
                btn,
              )}
            >
              取消
            </button>
            <button
              type="button"
              data-testid={`${p}-revise-submit`}
              disabled={disabled || trimmed === ''}
              title={blocked ?? (trimmed === '' ? '请先填写修改意见' : undefined)}
              onClick={() => void run('revise')}
              className={cn(
                'flex items-center gap-1 rounded-md border border-acc bg-acc text-fg-inv hover:bg-acc-soft disabled:cursor-not-allowed disabled:opacity-50',
                btn,
              )}
            >
              {submitting === 'revise' && <Loader2 size={11} className="animate-spin" />}
              {submitting === 'revise' ? '提交中…' : '提交'}
            </button>
          </div>
        </div>
      )}
      {note !== null && (
        <div data-testid={`${p}-decision-note`} className="text-[10.5px] leading-[15px] text-fg-3">
          {note}
        </div>
      )}
      {error !== null && (
        <div
          role="alert"
          data-testid={`${p}-decision-error`}
          className="rounded-md border border-warn/30 bg-warn-bg px-2 py-1.5 text-[11px] leading-[16px] text-warn"
        >
          {error}
        </div>
      )}
    </div>
  );
}
