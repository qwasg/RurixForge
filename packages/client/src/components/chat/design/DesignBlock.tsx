import { useEffect, useRef, useState } from 'react';
import { Check, ChevronDown, Layers, Loader2, MessageSquareText, Palette, RefreshCw, ScanSearch, TriangleAlert } from 'lucide-react';
import { cn } from '@/lib/cn';
import type { ChatBlock } from '@/lib/timeline';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import {
  designActionErrorText,
  designFileUrl,
  designPhaseText,
  useDesignFlowStore,
  type DesignAction,
  type DesignState,
} from '@/lib/designFlowStore';
import { modesForKind } from '../composerModes';
import ImageLightbox from './ImageLightbox';

type Block = Extract<ChatBlock, { kind: 'design' }>;

/**
 * D-045:Design 流程卡分发壳。审阅卡(挑选 / 采用 / 修改 / 重出)、元素清单卡(叠框图)、
 * 验收卡(截帧 vs 定稿滑块、差异热力图、逐元素得分)、完成卡(结论 + 提出修复)。
 * 动作一律带「这一张卡」的 {id, rev},过期由后端 409 兜底;前端先按同一套条件禁用并写明原因。
 */
export default function DesignBlock({ block }: { block: Block }) {
  const body = (() => {
    switch (block.step) {
      case 'review':
        return <ReviewCard key={`${block.flowId}:${block.rev}`} block={block} />;
      case 'layout':
        return <LayoutCard block={block} />;
      case 'verify':
        return <VerifyCard block={block} />;
      case 'done':
        return <DoneCard block={block} />;
      default:
        return null;
    }
  })();
  if (body === null) return null;
  return (
    <div data-testid="design-block" data-step={block.step} data-rev={block.rev} data-submitted={block.submitted ? '1' : undefined}>
      {body}
    </div>
  );
}

// ---------- 共用 ----------

interface Ctx {
  sessionId: string | null;
  flow: DesignState | null;
  activeRunId: string | null;
  pending: boolean;
  unsupported: string | null;
}

function useCtx(): Ctx {
  const sessionId = useSessionStore((st) => st.activeSessionId);
  const kind = useSessionStore((st) => st.sessions.find((x) => x.id === st.activeSessionId)?.agentKind ?? 'coding');
  const engine = useSessionStore(
    (st) => st.sessions.find((x) => x.id === st.activeSessionId)?.agentEngine ?? st.draftAgentEngine,
  );
  const state = useDesignFlowStore((st) => st.state);
  const flowSession = useDesignFlowStore((st) => st.sessionId);
  const pending = useDesignFlowStore((st) => st.pending !== null);
  const activeRunId = useChatStore((st) => st.activeRunId);
  const supported = modesForKind(kind, engine).some((m) => m.id === 'design');
  return {
    sessionId,
    flow: state !== null && flowSession === sessionId ? state : null,
    activeRunId,
    pending,
    unsupported: supported ? null : '当前代理类型不支持 Design,切回「编码」代理后可继续',
  };
}

/** 卡片动作为什么不能点(null = 可以)。 */
export function designBlockReason(
  block: { flowId: string; step: Block['step']; rev: number },
  ctx: Pick<Ctx, 'flow' | 'activeRunId' | 'pending' | 'unsupported'>,
): string | null {
  const flow = ctx.flow;
  if (!flow || flow.id !== block.flowId) return '该卡片不属于当前流程(流程可能已重新开始)';
  if (ctx.unsupported) return ctx.unsupported;
  if (block.step === 'review' && (flow.stage !== 'design_review' || flow.designRev !== block.rev)) {
    return '这一批设计稿已处理';
  }
  if (block.step === 'done' && !(flow.stage === 'done' && flow.replicationRound === block.rev)) {
    return '复刻已进入新一轮';
  }
  if (flow.phase === 'running') return designPhaseText(flow);
  if (ctx.activeRunId !== null) return '有任务正在运行';
  if (ctx.pending) return '上一个操作还在提交中';
  return null;
}

function rec(v: unknown): Record<string, unknown> | null {
  return v !== null && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
}

function Card({ icon, title, badge, children, testId }: { icon: React.ReactNode; title: string; badge?: React.ReactNode; children: React.ReactNode; testId: string }) {
  return (
    <div data-testid={testId} className="my-1.5 flex min-w-0 flex-col gap-2 rounded-lg border border-edge bg-shell-panel px-3 py-2.5">
      <div className="flex items-center gap-1.5 text-[12px] font-medium text-fg-2">
        {icon}
        <span className="min-w-0 flex-1 truncate">{title}</span>
        {badge}
      </div>
      {children}
    </div>
  );
}

function Badge({ ok, text }: { ok: boolean; text: string }) {
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center gap-0.5 rounded-full px-1.5 py-px text-[10px] leading-[15px]',
        ok ? 'bg-sage-bg text-sage' : 'bg-warn-bg text-warn',
      )}
    >
      {ok ? <Check size={10} /> : <TriangleAlert size={10} />}
      {text}
    </span>
  );
}

const btn = 'flex h-[24px] items-center gap-1 rounded-md px-2 text-[11px] disabled:cursor-not-allowed disabled:opacity-50';
const primary = `${btn} border border-acc bg-acc font-medium text-fg-inv hover:bg-acc-soft`;
const secondary = `${btn} border border-edge bg-shell-panel text-fg-2 hover:bg-shell-hover`;

/** 意见输入 + 提交(修改 / 重出 / 修复共用)。 */
function FeedbackBox({
  testId,
  placeholder,
  required,
  disabled,
  submitting,
  onSubmit,
  onCancel,
}: {
  testId: string;
  placeholder: string;
  required: boolean;
  disabled: boolean;
  submitting: boolean;
  onSubmit: (text: string) => void;
  onCancel: () => void;
}) {
  const [text, setText] = useState('');
  const trimmed = text.trim();
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <textarea
        data-testid={`${testId}-input`}
        rows={3}
        value={text}
        disabled={disabled}
        placeholder={placeholder}
        onChange={(e) => setText(e.target.value)}
        className="w-full resize-y rounded-md border border-edge bg-shell-panel px-2 py-1.5 text-[12px] leading-[18px] text-fg outline-none placeholder:text-fg-4 focus:border-acc-ring disabled:opacity-70"
      />
      <div className="flex items-center justify-end gap-1.5">
        <button type="button" className={cn(btn, 'text-fg-3 hover:bg-shell-hover')} onClick={onCancel}>
          取消
        </button>
        <button
          type="button"
          data-testid={`${testId}-submit`}
          disabled={disabled || (required && trimmed === '')}
          className={primary}
          onClick={() => onSubmit(trimmed)}
        >
          {submitting && <Loader2 size={11} className="animate-spin" />}
          提交
        </button>
      </div>
    </div>
  );
}

function useAct() {
  const [submitting, setSubmitting] = useState<DesignAction | null>(null);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const run = async (action: DesignAction, opts: { rev: number; candidate?: number; userInput?: string; id: string }) => {
    setSubmitting(action);
    setError(null);
    const r = await useDesignFlowStore.getState().act(action, opts);
    if (!alive.current) return r.ok;
    setSubmitting(null);
    if (!r.ok) setError(designActionErrorText(r.code, r.message));
    return r.ok;
  };
  return { submitting, error, run };
}

// ---------- 审阅卡 ----------

interface Candidate {
  index: number;
  path: string;
  prompt: string | null;
}

function readCandidates(payload: Record<string, unknown>): Candidate[] {
  const raw = Array.isArray(payload.candidates) ? payload.candidates : [];
  return raw
    .map(rec)
    .filter((c): c is Record<string, unknown> => c !== null && typeof c.index === 'number' && typeof c.path === 'string')
    .map((c) => ({ index: c.index as number, path: c.path as string, prompt: typeof c.prompt === 'string' ? c.prompt : null }));
}

const DECISION_TEXT: Record<string, string> = {
  approve_design: '已采用',
  revise_design: '已提出修改',
  regenerate_design: '已要求重新生成',
};

function ReviewCard({ block }: { block: Block }) {
  const ctx = useCtx();
  const candidates = readCandidates(block.payload);
  const flowSelected = ctx.flow?.id === block.flowId && ctx.flow.designRev === block.rev ? ctx.flow.selected : null;
  const [selected, setSelected] = useState<number | null>(flowSelected ?? candidates[0]?.index ?? null);
  const [mode, setMode] = useState<'revise' | 'regenerate' | null>(null);
  const [zoom, setZoom] = useState<Candidate | null>(null);
  const act = useAct();
  useEffect(() => {
    if (flowSelected !== null) setSelected(flowSelected);
  }, [flowSelected]);

  const decided = rec(block.submitted);
  const blocked = decided ? null : designBlockReason(block, ctx);
  const disabled = decided !== null || blocked !== null || act.submitting !== null;
  const summary = typeof block.payload.summary === 'string' ? block.payload.summary : '';
  const w = typeof block.payload.width === 'number' ? block.payload.width : null;
  const h = typeof block.payload.height === 'number' ? block.payload.height : null;
  const url = (p: string) => (ctx.sessionId ? designFileUrl(ctx.sessionId, p) : '');
  const pick = (index: number) => {
    if (disabled) return;
    setSelected(index);
    void useDesignFlowStore.getState().select(index, { id: block.flowId, rev: block.rev });
  };
  const target = { id: block.flowId, rev: block.rev };

  return (
    <Card
      testId="design-review-card"
      icon={<Palette size={13} className="text-acc" />}
      title={`设计稿 · 第 ${block.rev} 批${w && h ? ` · ${w}×${h}` : ''}`}
      badge={decided ? <Badge ok text={DECISION_TEXT[String(decided.action)] ?? '已处理'} /> : undefined}
    >
      {summary !== '' && <p className="whitespace-pre-wrap text-[11.5px] leading-[17px] text-fg-3">{summary}</p>}
      <div className={cn('grid gap-1.5', candidates.length > 1 ? 'grid-cols-2' : 'grid-cols-1')}>
        {candidates.map((c) => {
          const chosen = decided ? decided.candidate === c.index : selected === c.index;
          return (
            <div
              key={c.index}
              data-testid={`design-candidate-${c.index}`}
              data-selected={chosen ? '1' : '0'}
              className={cn(
                'group relative overflow-hidden rounded-md border-2 bg-shell-hover',
                chosen ? 'border-acc' : 'border-transparent',
                !disabled && 'cursor-pointer',
              )}
              onClick={() => pick(c.index)}
            >
              <img src={url(c.path)} alt={`候选 #${c.index}`} className="block h-auto w-full" loading="lazy" />
              <span className="absolute left-1 top-1 rounded bg-black/60 px-1 text-[10px] text-white">#{c.index}</span>
              <button
                type="button"
                aria-label={`放大候选 #${c.index}`}
                data-testid={`design-candidate-zoom-${c.index}`}
                className="absolute right-1 top-1 rounded bg-black/60 p-0.5 text-white opacity-0 transition-opacity group-hover:opacity-100"
                onClick={(e) => {
                  e.stopPropagation();
                  setZoom(c);
                }}
              >
                <ScanSearch size={12} />
              </button>
            </div>
          );
        })}
      </div>
      {decided ? (
        <p data-testid="design-review-decided" className="text-[11px] text-fg-3">
          {DECISION_TEXT[String(decided.action)] ?? '已处理'}
          {typeof decided.candidate === 'number' ? ` #${decided.candidate}` : ''}
          {typeof decided.feedback === 'string' && decided.feedback !== '' ? `:${decided.feedback}` : ''}
        </p>
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-1.5">
            <button
              type="button"
              data-testid="design-approve"
              disabled={disabled || selected === null}
              title={blocked ?? '采用选中的这一张,开始在引擎里复刻'}
              className={primary}
              onClick={() => selected !== null && void act.run('approve_design', { ...target, candidate: selected })}
            >
              {act.submitting === 'approve_design' ? <Loader2 size={11} className="animate-spin" /> : <Check size={11} />}
              采用并开始复刻
            </button>
            <button
              type="button"
              data-testid="design-revise-toggle"
              disabled={disabled || selected === null}
              title={blocked ?? '在选中的这一张上按意见修改'}
              className={secondary}
              onClick={() => setMode((m) => (m === 'revise' ? null : 'revise'))}
            >
              <MessageSquareText size={11} />
              提出修改
            </button>
            <button
              type="button"
              data-testid="design-regenerate-toggle"
              disabled={disabled}
              title={blocked ?? '都不满意,换个方向重出一批'}
              className={secondary}
              onClick={() => setMode((m) => (m === 'regenerate' ? null : 'regenerate'))}
            >
              <RefreshCw size={11} />
              重新生成
            </button>
          </div>
          {mode === 'revise' && (
            <FeedbackBox
              testId="design-revise"
              placeholder="想改哪里?例如:标题再大一点、按钮换成圆角、整体偏冷色…"
              required
              disabled={disabled}
              submitting={act.submitting === 'revise_design'}
              onCancel={() => setMode(null)}
              onSubmit={(text) =>
                selected !== null &&
                void act.run('revise_design', { ...target, candidate: selected, userInput: text }).then((ok) => ok && setMode(null))
              }
            />
          )}
          {mode === 'regenerate' && (
            <FeedbackBox
              testId="design-regenerate"
              placeholder="可选:补充新的方向,例如:换成像素风、竖屏布局…"
              required={false}
              disabled={disabled}
              submitting={act.submitting === 'regenerate_design'}
              onCancel={() => setMode(null)}
              onSubmit={(text) =>
                void act.run('regenerate_design', { ...target, userInput: text }).then((ok) => ok && setMode(null))
              }
            />
          )}
          {blocked !== null && (
            <div data-testid="design-review-note" className="text-[10.5px] leading-[15px] text-fg-3">
              {blocked}
            </div>
          )}
        </>
      )}
      {act.error !== null && (
        <div role="alert" className="rounded-md border border-warn/30 bg-warn-bg px-2 py-1.5 text-[11px] text-warn">
          {act.error}
        </div>
      )}
      {zoom && (
        <ImageLightbox src={url(zoom.path)} title={`候选 #${zoom.index}${zoom.prompt ? ` · ${zoom.prompt}` : ''}`} onClose={() => setZoom(null)} />
      )}
    </Card>
  );
}

// ---------- 元素清单卡 ----------

function LayoutCard({ block }: { block: Block }) {
  const ctx = useCtx();
  const [open, setOpen] = useState(false);
  const [zoom, setZoom] = useState(false);
  const overlay = typeof block.payload.overlay === 'string' ? block.payload.overlay : null;
  const elements = (Array.isArray(block.payload.elements) ? block.payload.elements : []).map(rec).filter((e): e is Record<string, unknown> => e !== null);
  const count = (src: string) => elements.filter((e) => e.source === src).length;
  const url = overlay && ctx.sessionId ? designFileUrl(ctx.sessionId, overlay) : null;
  return (
    <Card testId="design-layout-card" icon={<Layers size={13} className="text-acc" />} title={`元素清单 · ${elements.length} 个原子元素`}>
      {url && (
        <img
          src={url}
          alt="元素叠框"
          data-testid="design-layout-overlay"
          className="block h-auto w-full cursor-zoom-in rounded-md"
          onClick={() => setZoom(true)}
        />
      )}
      <p className="text-[11px] text-fg-3">
        切图 {count('crop')} · 重绘 {count('regen')} · 文字 {count('text')} · 背景 {count('cleanplate') + elements.filter((e) => e.kind === 'background' && e.source === 'crop').length}
      </p>
      <button type="button" className="flex items-center gap-1 text-left text-[10.5px] text-fg-3 hover:text-fg-2" onClick={() => setOpen((v) => !v)}>
        <ChevronDown size={10} className={cn('transition-transform', !open && '-rotate-90')} />
        元素明细
      </button>
      {open && (
        <ul data-testid="design-layout-list" className="flex max-h-48 flex-col gap-0.5 overflow-auto rounded-md border border-edge px-2 py-1.5">
          {elements.map((e) => (
            <li key={String(e.id)} className="truncate font-code text-[10.5px] text-fg-2">
              {String(e.id)} · {String(e.kind)}/{String(e.source)} · {JSON.stringify(e.bbox)}
              {typeof e.text === 'string' ? ` · “${e.text}”` : ''}
            </li>
          ))}
        </ul>
      )}
      {zoom && url && <ImageLightbox src={url} title="元素叠框(蓝=文字 橙=按钮 绿=其它)" onClose={() => setZoom(false)} />}
    </Card>
  );
}

// ---------- 验收卡 ----------

function shotPath(payload: Record<string, unknown>, name: string): string | null {
  const s = rec(rec(payload.screenshots)?.[name]);
  return typeof s?.path === 'string' ? s.path : null;
}

/** 截帧 vs 定稿:滑块左右对比;可切到差异热力图。 */
export function CompareSlider({ frame, mockup, diff }: { frame: string; mockup: string; diff: string | null }) {
  const [pos, setPos] = useState(50);
  const [showDiff, setShowDiff] = useState(false);
  const [zoom, setZoom] = useState<string | null>(null);
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      {showDiff && diff ? (
        <img src={diff} alt="差异热力图" className="block h-auto w-full cursor-zoom-in rounded-md" onClick={() => setZoom(diff)} />
      ) : (
        <div className="relative overflow-hidden rounded-md" data-testid="design-compare">
          <img src={mockup} alt="定稿" className="block h-auto w-full" />
          <img
            src={frame}
            alt="引擎截帧"
            className="absolute inset-0 block h-full w-full"
            style={{ clipPath: `inset(0 ${100 - pos}% 0 0)` }}
          />
          <div className="pointer-events-none absolute inset-y-0 w-px bg-white/80" style={{ left: `${pos}%` }} />
          <span className="absolute left-1 top-1 rounded bg-black/60 px-1 text-[10px] text-white">引擎</span>
          <span className="absolute right-1 top-1 rounded bg-black/60 px-1 text-[10px] text-white">定稿</span>
        </div>
      )}
      <div className="flex items-center gap-2">
        {!showDiff && (
          <input
            type="range"
            min={0}
            max={100}
            value={pos}
            aria-label="对比位置"
            data-testid="design-compare-slider"
            onChange={(e) => setPos(Number(e.target.value))}
            className="min-w-0 flex-1"
          />
        )}
        {diff && (
          <button type="button" data-testid="design-diff-toggle" className={secondary} onClick={() => setShowDiff((v) => !v)}>
            {showDiff ? '对比滑块' : '差异图'}
          </button>
        )}
        <button type="button" className={secondary} onClick={() => setZoom(frame)}>
          放大截帧
        </button>
      </div>
      {zoom && <ImageLightbox src={zoom} title="复刻对比" onClose={() => setZoom(null)} />}
    </div>
  );
}

function VerifyCard({ block }: { block: Block }) {
  const ctx = useCtx();
  const passed = block.payload.passed === true;
  const global = rec(block.payload.global);
  const failed = Array.isArray(block.payload.failed) ? block.payload.failed.map(String) : [];
  const problems = Array.isArray(block.payload.sceneProblems) ? block.payload.sceneProblems.map(String) : [];
  const url = (name: string) => {
    const p = shotPath(block.payload, name);
    return p && ctx.sessionId ? designFileUrl(ctx.sessionId, p) : null;
  };
  const [frame, mockup, diff] = [url('frame'), url('mockup'), url('diff')];
  return (
    <Card
      testId="design-verify-card"
      icon={<ScanSearch size={13} className="text-acc" />}
      title={`复刻验收 #${block.rev}`}
      badge={<Badge ok={passed} text={passed ? '通过' : '未通过'} />}
    >
      {frame && mockup && <CompareSlider frame={frame} mockup={mockup} diff={diff} />}
      {global && (
        <p className="text-[11px] tabular-nums text-fg-3">
          全局 SSIM {Number(global.ssim ?? 0).toFixed(3)}(≥{String(global.ssimMin ?? '')}) · 色差 {Number(global.colorDiff ?? 0).toFixed(1)}(≤
          {String(global.colorDiffMax ?? '')})
        </p>
      )}
      {failed.length > 0 && (
        <p data-testid="design-verify-failed" className="text-[11px] text-warn">
          未对上的元素:{failed.join('、')}
        </p>
      )}
      {problems.length > 0 && (
        <ul className="text-[11px] text-warn">
          {problems.map((p) => (
            <li key={p}>{p}</li>
          ))}
        </ul>
      )}
    </Card>
  );
}

// ---------- 完成卡 ----------

function DoneCard({ block }: { block: Block }) {
  const ctx = useCtx();
  const [fixing, setFixing] = useState(false);
  const act = useAct();
  const passed = block.payload.passed === true;
  const summary = typeof block.payload.summary === 'string' ? block.payload.summary : '';
  const scene = typeof block.payload.scenePath === 'string' ? block.payload.scenePath : null;
  const accepted = typeof block.payload.acceptedFailures === 'string' ? block.payload.acceptedFailures : null;
  const blocked = designBlockReason(block, ctx);
  return (
    <Card
      testId="design-done-card"
      icon={<Palette size={13} className="text-acc" />}
      title="复刻完成"
      badge={<Badge ok={passed} text={passed ? '验收通过' : '带未通过项收尾'} />}
    >
      {summary !== '' && <p className="whitespace-pre-wrap text-[11.5px] leading-[17px] text-fg-2">{summary}</p>}
      {scene && <p className="font-code text-[10.5px] text-fg-3">场景:{scene}</p>}
      {accepted && <p className="text-[11px] text-warn">未通过原因:{accepted}</p>}
      <div className="flex items-center gap-1.5">
        <button
          type="button"
          data-testid="design-fix-toggle"
          disabled={blocked !== null}
          title={blocked ?? '对复刻结果提意见,再修一轮'}
          className={secondary}
          onClick={() => setFixing((v) => !v)}
        >
          <MessageSquareText size={11} />
          提出修复
        </button>
      </div>
      {fixing && (
        <FeedbackBox
          testId="design-fix"
          placeholder="哪里还不像?例如:标题字体太细、底部按钮间距偏大…"
          required
          disabled={blocked !== null || act.submitting !== null}
          submitting={act.submitting === 'fix_replication'}
          onCancel={() => setFixing(false)}
          onSubmit={(text) =>
            void act
              .run('fix_replication', { id: block.flowId, rev: block.rev, userInput: text })
              .then((ok) => ok && setFixing(false))
          }
        />
      )}
      {act.error !== null && (
        <div role="alert" className="rounded-md border border-warn/30 bg-warn-bg px-2 py-1.5 text-[11px] text-warn">
          {act.error}
        </div>
      )}
    </Card>
  );
}
