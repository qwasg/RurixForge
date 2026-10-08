import { useEffect, useRef, useState, type ReactNode } from 'react';
import { CheckCircle2, FileText, ListChecks, Loader2, Play } from 'lucide-react';
import type { ChatBlock } from '@/lib/timeline';
import { cn } from '@/lib/cn';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useEditorStore } from '@/lib/editorStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useChatStore } from '@/lib/chatStore';
import {
  actionErrorText, clearAcceptanceDraft, loadAcceptanceDraft, saveAcceptanceDraft,
  stageLabel, useUltraPlanStore, normalizeDelivery,
  type AcceptanceDraft, type AcceptanceResult, type ManualCheck, type UltraPlanState,
} from '@/lib/ultraPlanStore';
import { useFlowContext } from './flowContext';

type Ultra = Extract<ChatBlock, { kind: 'ultraplan' }>;
const button = 'inline-flex min-h-[26px] items-center justify-center gap-1 rounded-md border border-edge bg-shell-panel px-2 py-1 text-[11.5px] text-fg-2 hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-50';
const primary = cn(button, 'border-acc bg-acc text-fg-inv hover:bg-acc-soft');
const noteStyle = 'text-[11px] leading-[17px] text-fg-3';
const inputStyle = 'w-full resize-y rounded-md border border-edge bg-shell-panel px-2 py-1.5 text-[12px] text-fg outline-none focus:border-acc-ring disabled:opacity-60';

function text(payload: Record<string, unknown>, key: string): string {
  return typeof payload[key] === 'string' ? payload[key] as string : '';
}

function Shell({ title, icon, testId, children }: { title: string; icon: ReactNode; testId: string; children: ReactNode }) {
  return <section data-testid={testId} className="my-1 overflow-hidden rounded-[10px] border border-edge-strong bg-shell-sunk">
    <div className="flex items-center gap-2 border-b border-edge px-3 py-2.5 text-[12px] font-medium text-fg">{icon}{title}</div>
    <div className="flex flex-col gap-2 px-3 py-2.5">{children}</div>
  </section>;
}

function ErrorNote({ value, testId }: { value: string | null; testId: string }) {
  return value ? <div role="alert" data-testid={testId} className="rounded-md border border-warn/30 bg-warn-bg px-2 py-1.5 text-[11px] text-warn">{value}</div> : null;
}

/** 关口、版本、会话与运行状态一起判定;历史卡绝不会操作当前流程。 */
export function workflowBlockReason(flow: UltraPlanState | null, id: string, rev: number, stage: 'plan_review' | 'acceptance', activeRunId: string | null, unsupported: string | null, pending: boolean): string | null {
  if (!flow || flow.id !== id) return '此流程属于其他会话,或已被重新开始';
  if ((stage === 'plan_review' ? flow.planRev : flow.acceptanceRound) !== rev) return '已有更新的版本,请使用最新卡片';
  if (unsupported) return unsupported;
  if (flow.stage !== stage) return `流程已进入「${stageLabel(flow.stage)}」阶段`;
  if (flow.phase === 'running' || activeRunId !== null) return '有任务正在运行';
  if (pending) return '上一个操作还在提交中';
  return null;
}

/** 制作动作只确认当前权限的逐项审批语义,不切换为 bypass。 */
function useProductionDecision(action: 'start' | 'fix', id: string, rev: number) {
  const { activeSessionId, flow } = useFlowContext();
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState(false);
  const [ack, setAck] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  useEffect(() => { if (flow?.phase === 'failed') setSent(false); }, [flow?.phase]);
  useEffect(() => { setAck(false); setError(null); setSent(false); }, [id, rev, activeSessionId]);

  const execute = async (acknowledge = false) => {
    if (busy || sent) return;
    const current = useUltraPlanStore.getState();
    if (useSessionStore.getState().activeSessionId !== activeSessionId || current.sessionId !== activeSessionId || current.state?.id !== id) return;
    setBusy(true);
    setError(null);
    const result = await (action === 'start' ? current.startProduction(acknowledge, { id, rev }) : current.fixProduction(acknowledge, { id, rev }));
    if (!alive.current) return;
    setBusy(false);
    if (result.ok) { setSent(true); setAck(false); }
    else if (result.code === 'ULTRAPLAN_NEEDS_BYPASS' && !acknowledge) setAck(true);
    else { setAck(false); setError(actionErrorText(result.code, result.message)); }
  };
  return { busy, sent, ack, error, execute, cancelAck: () => setAck(false) };
}

function ApprovalNotice({ decision, disabled, prefix }: { decision: ReturnType<typeof useProductionDecision>; disabled: boolean; prefix: string }) {
  if (!decision.ack) return null;
  return <div data-testid={`${prefix}-approval`} className="flex flex-col gap-2 rounded-md border border-warn/30 bg-warn-bg p-2 text-[11px] text-warn">
    <span>当前权限下制作期间的写入需要逐项确认,审批超时会中断。保留当前权限继续制作?</span>
    <div className="flex gap-2">
      <button type="button" className={button} onClick={decision.cancelAck} disabled={decision.busy} data-testid={`${prefix}-ack-cancel`}>取消</button>
      <button type="button" className={button} onClick={() => void decision.execute(true)} disabled={disabled || decision.busy} data-testid={`${prefix}-ack-confirm`}>仍然继续</button>
    </div>
  </div>;
}

/** 计划卡与 Plan 页签共用。Plan 页签可先保存,后端仍校验 plan hash。 */
export function PlanDecisionControls({ upId, rev, prefix = 'ultraplan-plan', blocked: extraBlocked = null, beforeStart }: {
  upId: string; rev: number; prefix?: string; blocked?: string | null; beforeStart?: () => Promise<boolean>;
}) {
  const { flow, activeRunId, unsupportedReason, activeSessionId } = useFlowContext();
  const pending = useUltraPlanStore((s) => s.pending !== null);
  const decision = useProductionDecision('start', upId, rev);
  const [revising, setRevising] = useState(false);
  const [feedback, setFeedback] = useState('');
  const [saving, setSaving] = useState(false);
  const [sentRevision, setSentRevision] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  useEffect(() => { if (flow?.phase === 'failed') setSentRevision(false); }, [flow?.phase]);
  const blocked = extraBlocked ?? workflowBlockReason(flow, upId, rev, 'plan_review', activeRunId, unsupportedReason, pending);
  const disabled = blocked !== null || decision.busy || decision.sent || saving || sentRevision;

  const start = async () => {
    if (disabled) return;
    if (beforeStart) {
      setSaving(true);
      const saved = await beforeStart();
      if (!alive.current) return;
      setSaving(false);
      if (!saved) { setError('计划未能保存,已取消制作'); return; }
    }
    await decision.execute();
  };
  const revise = async () => {
    if (disabled || feedback.trim() === '') return;
    setSaving(true); setError(null);
    const result = await useUltraPlanStore.getState().revisePlan(feedback, { id: upId, rev });
    if (!alive.current || useSessionStore.getState().activeSessionId !== activeSessionId) return;
    setSaving(false);
    if (result.ok) { setSentRevision(true); setRevising(false); }
    else setError(actionErrorText(result.code, result.message));
  };
  return <div className="flex min-w-0 flex-col gap-2" data-testid={`${prefix}-controls`}>
    <div className="flex flex-wrap gap-2">
      <button type="button" className={primary} disabled={disabled} title={blocked ?? '确认此版本的技术选型、制作步骤与验收检查点,启动 Team'} onClick={() => void start()} data-testid={`${prefix}-start`}>
        {decision.busy || saving ? <Loader2 size={12} className="animate-spin" /> : <Play size={12} />}确认并开始制作
      </button>
      <button type="button" className={button} disabled={disabled} title={blocked ?? undefined} onClick={() => setRevising((v) => !v)} data-testid={`${prefix}-revise-toggle`}>要求修改</button>
    </div>
    {revising && <div className="flex flex-col gap-2">
      <textarea aria-label="计划修改意见" rows={3} className={inputStyle} value={feedback} disabled={disabled} onChange={(e) => setFeedback(e.target.value)} placeholder="说明需要调整的技术选型、范围、分工或验证步骤…" data-testid={`${prefix}-feedback`} />
      <button type="button" className={button} disabled={disabled || feedback.trim() === ''} onClick={() => void revise()} data-testid={`${prefix}-revise-submit`}>提交修改意见</button>
    </div>}
    <ApprovalNotice decision={decision} disabled={blocked !== null} prefix={prefix} />
    {(blocked || decision.sent || sentRevision) && <p className={noteStyle} data-testid={`${prefix}-note`}>{blocked ?? '已提交,等待处理…'}</p>}
    <ErrorNote value={error ?? decision.error} testId={`${prefix}-error`} />
  </div>;
}

export function PlanReviewCard({ block }: { block: Ultra }) {
  const { flow, activeSessionId } = useFlowContext();
  const payload = block.payload;
  const path = text(payload, 'planPath');
  const roles = Array.isArray(payload.roles) ? payload.roles.filter((r): r is string => typeof r === 'string') : [];
  const backend = text(payload, 'renderBackend') || (flow?.id === block.upId ? flow.renderBackend : '');
  const counts = [['taskCount', '个任务'], ['automated', '项自动化验证'], ['manual', '个人工检查点']]
    .filter(([key]) => typeof payload[key] === 'number').map(([key, unit]) => `${payload[key]} ${unit}`);
  return <Shell testId="ultraplan-plan-card" icon={<FileText size={14} />} title={`制作计划${block.rev > 1 ? ` · 第 ${block.rev} 版` : ''}`}>
    {text(payload, 'name') && <p className="text-[13px] text-fg">{text(payload, 'name')}</p>}
    {text(payload, 'overview') && <p className={noteStyle}>{text(payload, 'overview')}</p>}
    <p className={noteStyle}>{[text(payload, 'gameMode').toUpperCase(), backend ? `游戏后端: ${backend}` : '', ...counts].filter(Boolean).join(' · ')}</p>
    {roles.length > 0 && <p className={noteStyle}>Team 分工: {roles.join('、')}</p>}
    {path && <button type="button" className={button} data-testid="ultraplan-plan-open" onClick={() => useWorkbenchStore.getState().openPlan(path, activeSessionId ? { upId: block.upId, sessionId: activeSessionId } : undefined)}>在 Plan 页签打开</button>}
    <p className={noteStyle}>确认后 Team 将持续制作并自动验证画面与玩法,在最终验收时请你试玩确认。</p>
    <PlanDecisionControls upId={block.upId} rev={block.rev} blocked={path ? null : '计划文件路径缺失,请重新生成计划'} />
  </Shell>;
}

function manualChecks(raw: unknown): ManualCheck[] {
  if (!Array.isArray(raw)) return [];
  const seen = new Set<string>();
  return raw.flatMap((item) => {
    if (!item || typeof item !== 'object') return [];
    const r = item as Record<string, unknown>;
    const id = text(r, 'id');
    if (!id || seen.has(id)) return [];
    seen.add(id);
    return [{ id, title: text(r, 'title') || id, steps: text(r, 'steps'), expected: text(r, 'expected'), required: r.required !== false }];
  });
}

/** Load the approved scene in its own workspace before entering play mode. */
function DeliveryControls({ block }: { block: Ultra }) {
  const { flow, activeSessionId, activeRunId } = useFlowContext();
  const delivery = useUltraPlanStore((s) => s.delivery);
  const activeWorkspaceId = useWorkspaceStore((s) => s.activeWorkspaceId);
  const owned = flow?.id === block.upId ? flow : null;
  const target = normalizeDelivery(block.payload.delivery) ?? (owned ? delivery : null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const reason = !owned ? '此游戏属于其他会话或已重新开始'
    : owned.workspaceId !== activeWorkspaceId ? '请切回此游戏所属的工作区'
      : block.step === 'acceptance' && owned.acceptanceRound !== block.rev ? '已有更新的验收轮次'
        : !['acceptance', 'done'].includes(owned.stage) ? '游戏仍在制作中'
          : activeRunId !== null || owned.phase === 'running' ? '有任务正在运行'
            : !target ? '交付入口尚未加载，请刷新流程状态' : null;
  const stillOwned = () => {
    const store = useUltraPlanStore.getState();
    return useSessionStore.getState().activeSessionId === activeSessionId && store.sessionId === activeSessionId
      && store.state?.id === block.upId && store.state.workspaceId === activeWorkspaceId
      && useWorkspaceStore.getState().activeWorkspaceId === activeWorkspaceId
      && ['acceptance', 'done'].includes(store.state.stage) && store.state.phase !== 'running'
      && useChatStore.getState().activeRunId === null
      && (block.step !== 'acceptance' || store.state.acceptanceRound === block.rev);
  };
  const play = async () => {
    if (reason || busy || !target || !stillOwned()) return;
    if (/[:]|^[\\/]|(^|[\\/])\.{1,2}([\\/]|$)/.test(target.entry)) { setError('游戏入口路径无效'); return; }
    setBusy(true); setError(null);
    try {
      await useEditorStore.getState().openScenePath(target.entry);
      if (!alive.current || !stillOwned()) return;
      const loaded = useEditorStore.getState();
      if (loaded.lastError || loaded.scenePath !== target.entry) { setError(loaded.lastError || '游戏入口未能加载'); return; }
      useWorkbenchStore.getState().openEditor();
      await loaded.playEnter();
      if (alive.current && stillOwned()) setError(useEditorStore.getState().lastError);
    } catch (err) {
      if (alive.current) setError(err instanceof Error ? err.message : String(err));
    } finally { if (alive.current) setBusy(false); }
  };
  const dir = owned?.dir || text(block.payload, 'dir');
  return <div className="flex flex-col gap-2" data-testid="ultraplan-delivery">
    {target && <><p className={`${noteStyle} break-all`}>游戏入口: {target.entry}</p><p className={`${noteStyle} whitespace-pre-wrap`}>操作说明: {target.controls}</p></>}
    <div className="flex flex-wrap gap-2">
      <button type="button" className={primary} disabled={!!reason || busy} title={reason ?? undefined} onClick={() => void play()} data-testid="ultraplan-play-game">{busy ? <Loader2 size={12} className="animate-spin" /> : <Play size={12} />}{block.step === 'done' ? '打开游戏' : '启动游戏试玩'}</button>
      {dir && owned && <button type="button" className={button} disabled={owned.workspaceId !== activeWorkspaceId} onClick={() => useWorkbenchStore.getState().openFile(`${dir}/production.json`)} data-testid="ultraplan-evidence">查看验证证据索引</button>}
    </div>
    {reason && <p className={noteStyle}>{reason}</p>}
    <ErrorNote value={error} testId="ultraplan-play-error" />
  </div>;
}

export function AcceptanceCard({ block }: { block: Ultra }) {
  const { flow, activeRunId, unsupportedReason, activeSessionId } = useFlowContext();
  const pending = useUltraPlanStore((s) => s.pending !== null);
  const savedRounds = useUltraPlanStore((s) => s.acceptance?.rounds);
  const checks = manualChecks(block.payload.manual);
  const recorded = block.submitted ?? (flow?.id === block.upId && Array.isArray(savedRounds) ? savedRounds.find((r) => r?.round === block.rev) : undefined);
  const savedResults: AcceptanceResult[] = Array.isArray(recorded?.results) ? recorded.results.flatMap((raw) => {
    if (!raw || typeof raw !== 'object') return [];
    const r = raw as Record<string, unknown>;
    return typeof r.id === 'string' && (r.status === 'pass' || r.status === 'fail' || r.status === 'skip')
      ? [{ id: r.id, status: r.status, ...(typeof r.note === 'string' ? { note: r.note } : {}) }] : [];
  }) : [];
  const [draft, setDraft] = useState<AcceptanceDraft>(() => loadAcceptanceDraft(block.upId, block.rev) ?? {});
  const [busy, setBusy] = useState(false);
  const [submitted, setSubmitted] = useState<AcceptanceResult[] | null>(null);
  const [needsFix, setNeedsFix] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const decision = useProductionDecision('fix', block.upId, block.rev);
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const blocked = workflowBlockReason(flow, block.upId, block.rev, 'acceptance', activeRunId, unsupportedReason, pending);
  const results = submitted ?? savedResults;
  const hasSubmitted = submitted !== null || recorded !== undefined;
  const failed = results.filter((r) => r.status === 'fail');
  const repair = hasSubmitted && (needsFix || failed.length > 0);
  const disabled = blocked !== null || busy || hasSubmitted;
  const valid = checks.length > 0 && checks.every((c) => {
    const a = draft[c.id];
    return a?.status === 'pass' || (a?.status === 'fail' && !!a.note?.trim()) || (c.required === false && a?.status === 'skip' && !!a.note?.trim());
  });
  const update = (id: string, patch: AcceptanceDraft[string]) => {
    if (disabled) return;
    const next = { ...draft, [id]: { ...draft[id], ...patch } };
    setDraft(next); saveAcceptanceDraft(block.upId, block.rev, next); setError(null);
  };
  const submit = async () => {
    if (disabled || !valid) return;
    const current = useUltraPlanStore.getState();
    if (current.sessionId !== activeSessionId || current.state?.id !== block.upId || current.state.acceptanceRound !== block.rev) return;
    const next: AcceptanceResult[] = checks.map((c) => ({ id: c.id, status: draft[c.id].status!, ...(draft[c.id].note?.trim() ? { note: draft[c.id].note!.trim() } : {}) }));
    setBusy(true); setError(null);
    const outcome = await useUltraPlanStore.getState().submitAcceptance(next, block.rev);
    if (!alive.current) return;
    setBusy(false);
    if (!outcome.ok) { setError(actionErrorText(outcome.code, outcome.message)); return; }
    clearAcceptanceDraft(block.upId, block.rev);
    setSubmitted(next);
    if (outcome.next === 'fix' && useSessionStore.getState().activeSessionId === activeSessionId && useUltraPlanStore.getState().state?.id === block.upId) {
      setNeedsFix(true);
      await decision.execute();
    }
  };
  return <Shell testId="ultraplan-acceptance-card" icon={<ListChecks size={14} />} title={`人工验收 · 第 ${block.rev} 轮`}>
    <p className={noteStyle}>请在游戏编辑器中试玩,逐项确认真实操作与游戏行为。未通过项提交后会自动交给 Team 修复。</p>
    <DeliveryControls block={block} />
    {checks.map((check, index) => {
      const answer = hasSubmitted ? results.find((r) => r.id === check.id) : draft[check.id];
      return <fieldset key={check.id} disabled={disabled} className="min-w-0 rounded-md border border-edge p-2" data-testid={`acceptance-${check.id}`}>
        <legend className="px-1 text-[12px] text-fg">{index + 1}. {check.title} <span className="text-[10px] text-fg-4">{check.required !== false ? '必验' : '可选'}</span></legend>
        <p className={`${noteStyle} whitespace-pre-wrap`}>操作: {check.steps || '按计划中的步骤试玩'}</p>
        <p className={`${noteStyle} whitespace-pre-wrap`}>预期: {check.expected || '符合计划定义的行为'}</p>
        {hasSubmitted ? <p className="mt-1 text-[12px] text-fg-2" data-testid={`acceptance-${check.id}-recorded`}>{answer?.status === 'pass' ? '通过' : answer?.status === 'fail' ? '未通过' : answer?.status === 'skip' ? '跳过' : '未记录'}{answer?.note ? `: ${answer.note}` : ''}</p> : <>
          <div role="group" aria-label={`${check.title}的验收结果`} className="my-2 flex flex-wrap gap-1.5">
            {(['pass', 'fail', 'skip'] as const).map((status) => <button key={status} type="button" className={`${button} aria-pressed:border-acc aria-pressed:bg-acc-bg aria-pressed:text-acc`} aria-pressed={answer?.status === status} disabled={disabled || (status === 'skip' && check.required !== false)} title={status === 'skip' && check.required !== false ? '必要检查点不能跳过' : undefined} onClick={() => update(check.id, { status })} data-testid={`acceptance-${check.id}-${status}`}>{status === 'pass' ? '通过' : status === 'fail' ? '未通过' : '跳过'}</button>)}
          </div>
          <textarea className={inputStyle} aria-label={`${check.title}的说明`} rows={2} placeholder={answer?.status === 'fail' ? '请描述未通过的现象(必填)' : answer?.status === 'skip' ? '请说明跳过原因(必填)' : '补充说明(可选)'} value={answer?.note ?? ''} onChange={(e) => update(check.id, { note: e.target.value })} data-testid={`acceptance-${check.id}-note`} />
        </>}
      </fieldset>;
    })}
    {checks.length === 0 && <p className={noteStyle}>验收检查点缺失,请刷新流程状态。</p>}
    {!hasSubmitted && <button type="button" className={primary} disabled={disabled || !valid} onClick={() => void submit()} data-testid="acceptance-submit">{busy && <Loader2 size={12} className="animate-spin" />}提交验收结果</button>}
    {hasSubmitted && <p className={noteStyle} data-testid="acceptance-submitted">已提交 · 通过 {results.filter((r) => r.status === 'pass').length} · 未通过 {failed.length} · 跳过 {results.filter((r) => r.status === 'skip').length}</p>}
    {repair && !decision.sent && <button type="button" className={primary} disabled={blocked !== null || decision.busy || busy} onClick={() => void decision.execute()} data-testid="acceptance-retry-fix">{decision.busy && <Loader2 size={12} className="animate-spin" />}继续修复未通过项</button>}
    <ApprovalNotice decision={decision} disabled={blocked !== null} prefix="acceptance" />
    {blocked && <p className={noteStyle} data-testid="acceptance-blocked">{blocked}</p>}
    <ErrorNote value={error ?? decision.error} testId="acceptance-error" />
  </Shell>;
}

export function DoneCard({ block }: { block: Ultra }) {
  const { flow, activeSessionId } = useFlowContext();
  const current = flow?.id === block.upId && flow.stage === 'done' ? flow : null;
  const planPath = text(block.payload, 'planPath') || current?.planPath;
  const dir = text(block.payload, 'dir') || current?.dir;
  return <Shell testId="ultraplan-done-card" icon={<CheckCircle2 size={14} />} title="UltraPlan 已完成">
    <p className={noteStyle}>制作与验收记录已保存。游戏源码和资产保留在项目工作区。</p>
    {dir && <p className={`${noteStyle} break-all`}>需求、演示与验收记录: {dir}</p>}
    <DeliveryControls block={block} />
    {planPath && activeSessionId && <button type="button" className={button} onClick={() => useWorkbenchStore.getState().openPlan(planPath, { upId: block.upId, sessionId: activeSessionId })}>查看制作计划</button>}
  </Shell>;
}
