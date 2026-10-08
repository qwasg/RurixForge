import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { Check, ChevronDown, ChevronLeft, ChevronRight, ClipboardList, Loader2, Sparkles } from 'lucide-react';
import type { ChatBlock } from '@/lib/timeline';
import { cn } from '@/lib/cn';
import {
  actionErrorText,
  cardInteractive,
  clearDraft,
  loadDraft,
  saveDraft,
  stageIndex,
  stageLabel,
  useUltraPlanStore,
  type QuestionOption,
  type Questionnaire,
  type QuestionnaireQuestion,
  type UltraPlanAnswer,
  type UltraPlanAnswers,
  type UltraPlanState,
} from '@/lib/ultraPlanStore';
import MarkdownFlat from '../MarkdownFlat';
import { useFlowContext } from './flowContext';
import {
  DELEGATED_TEXT,
  SCALE_BUTTON_LIMIT,
  answerStatus,
  answerSummary,
  buildAnswers,
  canDelegate,
  delegatableRest,
  delegateRest,
  hasGap,
  isRequired,
  multiBounds,
  multiCount,
  normalizeQuestionnaire,
  sanitizeDraft,
  scaleRange,
} from './questionnaireModel';

type Ultra = Extract<ChatBlock, { kind: 'ultraplan' }>;

const RUNNING_NOTE = '有任务正在运行,结束后可继续填写';

/**
 * D-044:UltraPlan 问卷卡(Claude Design 式的需求问卷,但住在 300–560px 宽的对话列里)。
 *
 * 版式:单列;一次只展开一节(上一节 / 下一节 + 分节圆点),头部「已答 x/y」;每题
 * 标签(必答带 *)+ 说明 + 按 kind 的控件(single 单选卡 / multi 多选卡 / text 文本框 /
 * scale 可换行的分段按钮)+「其他…」自填 +「交给你决定」;底部「其余全部交给你决定」与「提交」。
 *
 * 三种形态:
 * - 表单:这张卡是当前流程、当前关口、当前版的问卷(契约 §9)。只要其中任一条件被一个
 *   正在跑的任务或不支持的引擎 / 代理临时挡住,表单原样留着但整体禁用,并说明原因——
 *   填到一半的草稿不因为后台来了一轮回执唤醒就从眼前消失。
 * - 已提交:block.submitted 有值(ultraplan.answers.submitted 回填),只读回显当时的答案。
 * - 只读说明:旧版问卷 / 别的流程 / 关口已过,给一句原因。
 *
 * 草稿:挂载时从 localStorage 读,每次改动即存,提交被受理后清(切会话 / 刷新不丢)。
 * 提交走 ultraPlanStore.submitAnswers(带这张卡自己的 id + rev),被拒的文案就地显示(warn 色)。
 */
export default function QuestionnaireCard({ block }: { block: Ultra }) {
  const { flow, activeRunId, unsupportedReason } = useFlowContext();
  const questionnaire = useMemo(
    () => normalizeQuestionnaire(block.payload.questionnaire),
    [block.payload],
  );
  const title = questionnaire?.title ?? '';

  if (block.submitted) {
    return (
      <CardShell rev={block.rev} title={title} mode="submitted" right={<Chip>已提交</Chip>}>
        {questionnaire && <Understanding text={questionnaire.understanding} defaultOpen={false} />}
        <SubmittedAnswers questionnaire={questionnaire} submitted={block.submitted} />
      </CardShell>
    );
  }

  const current =
    flow !== null &&
    block.upId === flow.id &&
    flow.stage === 'questionnaire' &&
    block.rev === flow.questionnaireRev;

  if (!current || !questionnaire || questionnaire.sections.length === 0) {
    const note = current ? '问卷内容缺失,请在输入框补充说明以重新生成' : staleReason(block, flow);
    return (
      <CardShell rev={block.rev} title={title} mode="readonly">
        {questionnaire && <Understanding text={questionnaire.understanding} defaultOpen={false} />}
        <div data-testid="questionnaire-note" className="border-t border-edge px-3 py-2 text-[11px] text-fg-3">
          {note}
        </div>
      </CardShell>
    );
  }

  // 契约 §9 的判据;此处 current 已成立,剩下能挡住它的只有「在跑」与「引擎 / 代理不支持」。
  const interactive = cardInteractive(block, flow, activeRunId) && unsupportedReason === null;
  const locked = interactive ? null : (unsupportedReason ?? RUNNING_NOTE);
  return (
    <QuestionnaireForm
      block={block}
      questionnaire={questionnaire}
      locked={locked}
      failed={flow.phase === 'failed'}
    />
  );
}

/** 不是当前可填的那张卡:为什么。 */
function staleReason(block: Ultra, flow: UltraPlanState | null): string {
  if (!flow || block.upId !== flow.id) return '此流程属于其他会话,或已被重新开始';
  if (block.rev < flow.questionnaireRev) return '已有更新的问卷';
  // 卡片比本地阶段新:建卡事件先到、阶段重拉还没回来(一瞬间),别误报成「关口已过」。
  if (block.rev > flow.questionnaireRev) return '正在同步流程状态…';
  if (flow.stage !== 'questionnaire') {
    return stageIndex(flow.stage) > stageIndex('questionnaire')
      ? `流程已进入「${stageLabel(flow.stage)}」阶段`
      : '需求正在重新梳理,稍后会有新的问卷';
  }
  return '正在同步流程状态…';
}

function Chip({ children, testId }: { children: ReactNode; testId?: string }) {
  return (
    <span
      data-testid={testId}
      className="shrink-0 rounded-full border border-edge bg-shell-panel px-1.5 py-0.5 text-[10px] leading-[14px] text-fg-3"
    >
      {children}
    </span>
  );
}

function CardShell({
  rev,
  title,
  mode,
  right,
  interactive,
  children,
}: {
  rev: number;
  title: string;
  mode: 'form' | 'submitted' | 'readonly';
  right?: ReactNode;
  interactive?: boolean;
  children: ReactNode;
}) {
  return (
    <section
      data-testid="questionnaire-card"
      data-mode={mode}
      data-interactive={interactive ? '1' : undefined}
      aria-label={title !== '' ? `需求问卷:${title}` : '需求问卷'}
      className="my-1 overflow-hidden rounded-[10px] border border-edge-strong bg-shell-sunk shadow-sh1"
    >
      <div className="flex items-start gap-2.5 px-3 py-2.5">
        <span className="mt-px flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-acc-bg text-acc">
          <ClipboardList size={13} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[10.5px] leading-[15px] text-fg-4">
            需求问卷{rev > 1 ? ` · 第 ${rev} 版` : ''}
          </div>
          {title !== '' && (
            <div className="break-words text-[12.5px] font-medium leading-[18px] text-fg">{title}</div>
          )}
        </div>
        {right}
      </div>
      {children}
    </section>
  );
}

/** 可折叠的「项目理解」(模型对设想的理解、假设与项目调研结论,markdown)。 */
function Understanding({ text, defaultOpen }: { text: string; defaultOpen: boolean }) {
  const [open, setOpen] = useState(defaultOpen);
  if (text.trim() === '') return null;
  return (
    <div className="border-t border-edge px-3 py-2">
      <button
        type="button"
        aria-expanded={open}
        data-testid="questionnaire-understanding-toggle"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-1 text-left text-[11px] font-medium text-fg-3 hover:text-fg-2"
      >
        <ChevronDown size={11} className={cn('shrink-0 transition-transform', !open && '-rotate-90')} />
        项目理解
      </button>
      {open && (
        <div
          data-testid="questionnaire-understanding"
          className="mt-1.5 max-h-60 overflow-auto rounded-md border border-edge bg-shell-panel px-2 py-1.5"
        >
          <MarkdownFlat text={text} />
        </div>
      )}
    </div>
  );
}

/** 已提交的答案:一题一行(选项 label / 「由 AI 决定」),高度封顶可滚。 */
function SubmittedAnswers({
  questionnaire,
  submitted,
}: {
  questionnaire: Questionnaire | null;
  submitted: Record<string, unknown>;
}) {
  const raw = submitted.answers;
  const answers: Record<string, unknown> =
    raw !== null && typeof raw === 'object' && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {};
  const questions = questionnaire?.sections.flatMap((s) => s.questions) ?? [];
  const delegated = questions.filter(
    (q) => (answers[q.id] as UltraPlanAnswer | undefined)?.delegate === true,
  ).length;
  return (
    <div data-testid="questionnaire-submitted" className="border-t border-edge px-3 py-2">
      <div className="text-[11px] text-fg-3">
        已提交
        {questions.length > 0 && ` · 共 ${questions.length} 题`}
        {delegated > 0 && `,其中 ${delegated} 题${DELEGATED_TEXT}`}
      </div>
      {questions.length > 0 && (
        <ol className="mt-1.5 flex max-h-64 flex-col gap-1.5 overflow-auto">
          {questions.map((q, i) => {
            const isDelegated = (answers[q.id] as UltraPlanAnswer | undefined)?.delegate === true;
            return (
              <li key={q.id} data-testid={`q-${q.id}-submitted`} className="flex gap-1.5 text-[11.5px] leading-[17px]">
                <span className="shrink-0 font-code text-[10.5px] text-fg-4">{i + 1}.</span>
                <span className="min-w-0 flex-1">
                  <span className="block break-words text-fg-3">{q.question}</span>
                  <span className={cn('block break-words', isDelegated ? 'text-fg-3' : 'text-fg')}>
                    {answerSummary(q, answers[q.id])}
                  </span>
                </span>
              </li>
            );
          })}
        </ol>
      )}
    </div>
  );
}

interface Rejection {
  message: string;
  questionId: string | null;
}

function QuestionnaireForm({
  block,
  questionnaire,
  locked,
  failed,
}: {
  block: Ultra;
  questionnaire: Questionnaire;
  /** 非 null = 表单整体禁用,值是原因。 */
  locked: string | null;
  /** 流程上一轮失败(phase=failed):已受理但没落库的提交要放开重填。 */
  failed: boolean;
}) {
  const pending = useUltraPlanStore((st) => st.pending);
  const submitAnswers = useUltraPlanStore((st) => st.submitAnswers);
  const sections = questionnaire.sections;
  const questions = useMemo(() => sections.flatMap((s) => s.questions), [sections]);

  const [answers, setAnswers] = useState<UltraPlanAnswers>(() =>
    sanitizeDraft(questionnaire, loadDraft(block.upId, block.rev)),
  );
  // 有草稿(刷新 / 切会话回来)→ 直接落到第一处还没填完的那一节;全新问卷从第一节开始。
  const [sectionIdx, setSectionIdx] = useState(() => {
    if (Object.keys(answers).length === 0) return 0;
    const gap = sections.findIndex((s) => s.questions.some((q) => hasGap(q, answers[q.id])));
    return gap >= 0 ? gap : sections.length - 1;
  });
  const [submitting, setSubmitting] = useState(false);
  /** 已被后端受理,等 answers.submitted 事件把卡片翻成只读。 */
  const [sent, setSent] = useState(false);
  const [rejection, setRejection] = useState<Rejection | null>(null);
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

  const inFlight =
    submitting || (pending !== null && pending.action === 'answer' && pending.rev === block.rev);
  const disabled = locked !== null || inFlight || sent || pending !== null;

  const commit = (next: UltraPlanAnswers) => {
    // 禁用态靠外层 <fieldset disabled> 让控件失效;这里再挡一道——脚本派发的 click、
    // 辅助技术的激活不一定尊重祖先 fieldset 的 disabled,锁住时答案不能被改动。
    if (disabled) return;
    setAnswers(next);
    setRejection(null);
    if (Object.keys(next).length === 0) clearDraft(block.upId, block.rev);
    else saveDraft(block.upId, block.rev, next);
  };
  const setAnswer = (id: string, value: UltraPlanAnswer | null) => {
    const next = { ...answers };
    if (value === null) delete next[id];
    else next[id] = value;
    commit(next);
  };

  const gaps = sections.map((s) => s.questions.filter((q) => hasGap(q, answers[q.id])).length);
  const firstGap = gaps.findIndex((n) => n > 0);
  const gapSections = gaps.filter((n) => n > 0).length;
  const answered = questions.filter((q) => answerStatus(q, answers[q.id]) === 'ok').length;
  const rest = delegatableRest(questionnaire, answers).length;
  const canSubmit = !disabled && firstGap < 0;
  const section = sections[Math.min(sectionIdx, sections.length - 1)];
  const offset = sections.slice(0, sectionIdx).reduce((n, s) => n + s.questions.length, 0);

  const gapText =
    firstGap < 0
      ? null
      : sections.length === 1
        ? `还有 ${gaps[firstGap]} 题未完成`
        : `第 ${firstGap + 1} 节「${sections[firstGap].title}」还有 ${gaps[firstGap]} 题未完成` +
          (gapSections > 1 ? `,另有 ${gapSections - 1} 节待补` : '');

  const submit = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    setRejection(null);
    const result = await submitAnswers(buildAnswers(questionnaire, answers), {
      id: block.upId,
      rev: block.rev,
    });
    // 受理即清草稿(卡片此刻可能已随切会话卸载,清理不依赖它还活着)。
    if (result.ok) clearDraft(block.upId, block.rev);
    if (!alive.current) return;
    setSubmitting(false);
    if (result.ok) {
      setSent(true);
      return;
    }
    const questionId = typeof result.details?.questionId === 'string' ? result.details.questionId : null;
    setRejection({ message: actionErrorText(result.code, result.message), questionId });
    if (questionId) {
      const at = sections.findIndex((s) => s.questions.some((q) => q.id === questionId));
      if (at >= 0) setSectionIdx(at);
    }
  };

  return (
    <CardShell
      rev={block.rev}
      title={questionnaire.title}
      mode="form"
      interactive={!disabled}
      right={
        <Chip testId="questionnaire-progress">
          已答 {answered}/{questions.length}
        </Chip>
      }
    >
      <Understanding text={questionnaire.understanding} defaultOpen />
      <fieldset
        disabled={disabled}
        className={cn('m-0 min-w-0 border-0 border-t border-edge px-3 py-2.5', disabled && 'opacity-70')}
      >
        <legend className="sr-only">{section.title}</legend>
        <div data-testid="questionnaire-section-title" className="mb-2 flex items-baseline gap-1.5">
          {sections.length > 1 && (
            <span className="shrink-0 font-code text-[10px] text-fg-4">
              {sectionIdx + 1}/{sections.length}
            </span>
          )}
          <span className="min-w-0 break-words text-[12px] font-semibold text-fg-2">{section.title}</span>
        </div>
        <div className="flex flex-col gap-3.5">
          {section.questions.map((q, i) => (
            <QuestionField
              key={q.id}
              q={q}
              index={offset + i + 1}
              answer={answers[q.id]}
              flagged={rejection?.questionId === q.id}
              onChange={(value) => setAnswer(q.id, value)}
            />
          ))}
        </div>
      </fieldset>

      {sections.length > 1 && (
        <div className="flex items-center gap-1.5 border-t border-edge px-3 py-2">
          <button
            type="button"
            data-testid="questionnaire-prev"
            disabled={sectionIdx === 0}
            onClick={() => setSectionIdx((v) => Math.max(0, v - 1))}
            className="flex h-[24px] shrink-0 items-center gap-0.5 rounded-md border border-edge px-1.5 text-[11px] text-fg-2 hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-40"
          >
            <ChevronLeft size={11} />
            上一节
          </button>
          <div
            role="group"
            aria-label="问卷分节"
            className="flex min-w-0 flex-1 flex-wrap items-center justify-center gap-1"
          >
            {sections.map((s, i) => (
              <button
                key={s.id}
                type="button"
                data-testid={`questionnaire-section-${i}`}
                data-state={gaps[i] === 0 ? 'done' : 'todo'}
                aria-current={i === sectionIdx ? 'step' : undefined}
                aria-label={`第 ${i + 1} 节 ${s.title}${gaps[i] > 0 ? `,还有 ${gaps[i]} 题未完成` : ',已完成'}`}
                title={`${s.title}${gaps[i] > 0 ? `(还有 ${gaps[i]} 题未完成)` : ''}`}
                onClick={() => setSectionIdx(i)}
                className={cn(
                  'flex h-[18px] min-w-[18px] items-center justify-center rounded-full border px-1 font-code text-[10px] transition-colors',
                  i === sectionIdx
                    ? 'border-acc bg-acc text-fg-inv'
                    : gaps[i] === 0
                      ? 'border-transparent bg-sage-bg text-sage'
                      : 'border-edge text-fg-3 hover:bg-shell-hover',
                )}
              >
                {i + 1}
              </button>
            ))}
          </div>
          <button
            type="button"
            data-testid="questionnaire-next"
            disabled={sectionIdx >= sections.length - 1}
            onClick={() => setSectionIdx((v) => Math.min(sections.length - 1, v + 1))}
            className="flex h-[24px] shrink-0 items-center gap-0.5 rounded-md border border-edge px-1.5 text-[11px] text-fg-2 hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-40"
          >
            下一节
            <ChevronRight size={11} />
          </button>
        </div>
      )}

      <div className="flex flex-col gap-1.5 border-t border-edge px-3 py-2">
        {(locked !== null || sent) && (
          <div data-testid="questionnaire-note" className="text-[11px] text-fg-3">
            {sent ? '已提交,等待处理…' : locked}
          </div>
        )}
        {rejection && (
          <div
            role="alert"
            data-testid="questionnaire-error"
            className="rounded-md border border-warn/30 bg-warn-bg px-2 py-1.5 text-[11px] leading-[16px] text-warn"
          >
            {rejection.message}
          </div>
        )}
        {gapText !== null && (
          <button
            type="button"
            data-testid="questionnaire-gap"
            onClick={() => setSectionIdx(firstGap)}
            className="self-start text-left text-[10.5px] leading-[15px] text-fg-3 underline-offset-2 hover:text-fg-2 hover:underline"
          >
            {gapText}
          </button>
        )}
        <div className="flex flex-wrap items-center justify-end gap-1.5">
          <button
            type="button"
            data-testid="questionnaire-delegate-rest"
            disabled={disabled || rest === 0}
            title={rest === 0 ? '没有可以交给 AI 决定的未答题' : `把剩下的 ${rest} 题交给 AI 决定`}
            onClick={() => commit(delegateRest(questionnaire, answers))}
            className="flex h-[25px] items-center gap-1 rounded-md border border-edge bg-shell-panel px-2 text-[11px] text-fg-2 hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-50"
          >
            <Sparkles size={11} />
            其余全部交给你决定
          </button>
          <button
            type="button"
            data-testid="questionnaire-submit"
            disabled={!canSubmit}
            title={canSubmit ? undefined : (locked ?? gapText ?? undefined)}
            onClick={() => void submit()}
            className="flex h-[25px] items-center gap-1 rounded-md border border-acc bg-acc px-2.5 text-[11px] text-fg-inv hover:bg-acc-soft disabled:cursor-not-allowed disabled:opacity-50"
          >
            {inFlight && <Loader2 size={11} className="animate-spin" />}
            {inFlight ? '提交中…' : '提交'}
          </button>
        </div>
      </div>
    </CardShell>
  );
}

/** radiogroup 内方向键:移动焦点并选中(WAI-ARIA 单选组的键盘约定);Tab / 空格 / 回车走原生按钮行为。 */
function onRadioKeys(event: KeyboardEvent<HTMLDivElement>) {
  const delta =
    event.key === 'ArrowDown' || event.key === 'ArrowRight'
      ? 1
      : event.key === 'ArrowUp' || event.key === 'ArrowLeft'
        ? -1
        : 0;
  if (delta === 0) return;
  const radios = Array.from(
    event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="radio"]:not(:disabled)'),
  );
  const at = radios.indexOf(document.activeElement as HTMLButtonElement);
  if (at < 0) return;
  event.preventDefault();
  const next = radios[(at + delta + radios.length) % radios.length];
  next.focus();
  next.click();
}

function QuestionField({
  q,
  index,
  answer,
  flagged,
  onChange,
}: {
  q: QuestionnaireQuestion;
  /** 全卷题号(1 起)。 */
  index: number;
  answer: UltraPlanAnswer | undefined;
  /** 后端点名这道题不合法(ULTRAPLAN_ANSWERS_INVALID.details.questionId)。 */
  flagged: boolean;
  /** null = 清掉这道题的答案。委托与具体答案互斥:任何一次具体作答都整条替换。 */
  onChange: (value: UltraPlanAnswer | null) => void;
}) {
  const labelId = useId();
  const required = isRequired(q);
  const delegated = answer?.delegate === true;
  const concrete = delegated ? undefined : answer;
  const options = q.options ?? [];
  const choice = concrete?.choice ?? [];
  const other = concrete?.other ?? '';

  let control: ReactNode = null;
  if (q.kind === 'single') {
    control = (
      <div role="radiogroup" aria-labelledby={labelId} onKeyDown={onRadioKeys} className="flex flex-col gap-1">
        {options.map((opt) => (
          <OptionCard
            key={opt.id}
            qid={q.id}
            opt={opt}
            role="radio"
            checked={choice.includes(opt.id)}
            onClick={() => onChange({ choice: [opt.id] })}
          />
        ))}
      </div>
    );
  } else if (q.kind === 'multi') {
    const { min, max } = multiBounds(q);
    const full = multiCount(q, concrete) >= max;
    const toggle = (id: string) => {
      const next = choice.includes(id) ? choice.filter((x) => x !== id) : [...choice, id];
      if (next.length === 0 && other.trim() === '') onChange(null);
      else onChange({ ...(next.length > 0 ? { choice: next } : {}), ...(other !== '' ? { other } : {}) });
    };
    control = (
      <div role="group" aria-labelledby={labelId} className="flex flex-col gap-1">
        {options.map((opt) => {
          const checked = choice.includes(opt.id);
          return (
            <OptionCard
              key={opt.id}
              qid={q.id}
              opt={opt}
              role="checkbox"
              checked={checked}
              blocked={!checked && full}
              blockedTitle={`最多选 ${max} 项`}
              onClick={() => toggle(opt.id)}
            />
          );
        })}
        <div data-testid={`q-${q.id}-bounds`} className="text-[10px] leading-[14px] text-fg-4">
          {min === max ? `选 ${min} 项` : `选 ${min}–${max} 项`}
          {!delegated && ` · 已选 ${multiCount(q, concrete)}`}
        </div>
      </div>
    );
  } else if (q.kind === 'text') {
    control = (
      <textarea
        data-testid={`q-${q.id}-text`}
        aria-labelledby={labelId}
        rows={2}
        value={concrete?.text ?? ''}
        placeholder={required ? '请填写' : '选填'}
        onChange={(e) => onChange(e.target.value === '' ? null : { text: e.target.value })}
        className="w-full resize-y rounded-md border border-edge bg-shell-panel px-2 py-1.5 text-[12px] leading-[18px] text-fg outline-none placeholder:text-fg-4 focus:border-acc-ring"
      />
    );
  } else {
    const { min, max } = scaleRange(q);
    const count = max - min + 1;
    control = (
      <div className="flex flex-col gap-1">
        {count <= SCALE_BUTTON_LIMIT ? (
          <div role="radiogroup" aria-labelledby={labelId} onKeyDown={onRadioKeys} className="flex flex-wrap gap-1">
            {Array.from({ length: count }, (_, i) => min + i).map((n) => {
              const checked = concrete?.scale === n;
              return (
                <button
                  key={n}
                  type="button"
                  role="radio"
                  aria-checked={checked}
                  data-testid={`q-${q.id}-scale-${n}`}
                  onClick={() => onChange({ scale: n })}
                  className={cn(
                    'h-[26px] min-w-[30px] rounded-md border px-1.5 font-code text-[11.5px] transition-colors disabled:cursor-not-allowed',
                    checked
                      ? 'border-acc-ring bg-acc-bg text-acc'
                      : 'border-edge bg-shell-panel text-fg-2 hover:bg-shell-hover',
                  )}
                >
                  {n}
                </button>
              );
            })}
          </div>
        ) : (
          <input
            type="number"
            data-testid={`q-${q.id}-scale-input`}
            aria-labelledby={labelId}
            min={min}
            max={max}
            step={1}
            value={concrete?.scale ?? ''}
            placeholder={`${min}–${max}`}
            onChange={(e) => {
              const n = Number(e.target.value);
              onChange(e.target.value !== '' && Number.isInteger(n) && n >= min && n <= max ? { scale: n } : null);
            }}
            className="w-28 rounded-md border border-edge bg-shell-panel px-2 py-1 font-code text-[12px] text-fg outline-none placeholder:text-fg-4 focus:border-acc-ring"
          />
        )}
        {q.scaleLabels && (
          <div className="flex justify-between gap-3 text-[10px] leading-[14px] text-fg-4">
            <span data-testid={`q-${q.id}-scale-label-min`}>
              {min} · {q.scaleLabels[0]}
            </span>
            <span data-testid={`q-${q.id}-scale-label-max`} className="text-right">
              {q.scaleLabels[1]} · {max}
            </span>
          </div>
        )}
      </div>
    );
  }

  return (
    <div
      data-testid={`q-${q.id}`}
      data-status={answerStatus(q, answer)}
      data-flagged={flagged ? '1' : undefined}
      className={cn('flex flex-col gap-1.5', flagged && '-mx-1.5 rounded-md border border-warn/30 bg-warn-bg px-1.5 py-1.5')}
    >
      <div className="flex items-start gap-1.5">
        <span className="shrink-0 font-code text-[10.5px] leading-[18px] text-fg-4">{index}.</span>
        <div className="min-w-0 flex-1">
          <div id={labelId} className="break-words text-[12.5px] font-medium leading-[18px] text-fg">
            {q.question}
            {required && (
              <>
                <span aria-hidden="true" className="text-fg-3"> *</span>
                <span className="sr-only">(必答)</span>
              </>
            )}
          </div>
          {q.help && <div className="mt-0.5 break-words text-[10.5px] leading-[15px] text-fg-4">{q.help}</div>}
        </div>
      </div>
      {control}
      {q.allowOther && (q.kind === 'single' || q.kind === 'multi') && (
        <input
          type="text"
          data-testid={`q-${q.id}-other`}
          aria-label={`${q.question}:其他`}
          value={other}
          placeholder="其他…(自行填写)"
          disabled={q.kind === 'multi' && other.trim() === '' && multiCount(q, concrete) >= multiBounds(q).max}
          onChange={(e) => {
            const text = e.target.value;
            // single:选项与「其他」二选一;multi:「其他」与已选项并存。
            const kept = q.kind === 'multi' && choice.length > 0 ? { choice } : {};
            if (text === '' && choice.length === 0) onChange(null);
            else if (text === '') onChange(q.kind === 'multi' ? kept : { choice });
            else onChange({ ...kept, other: text });
          }}
          className={cn(
            'w-full rounded-md border bg-shell-panel px-2 py-1.5 text-[12px] text-fg outline-none placeholder:text-fg-4 focus:border-acc-ring disabled:cursor-not-allowed',
            other.trim() !== '' ? 'border-acc-ring' : 'border-edge',
          )}
        />
      )}
      {canDelegate(q) && (
        <button
          type="button"
          aria-pressed={delegated}
          data-testid={`q-${q.id}-delegate`}
          onClick={() => onChange(delegated ? null : { delegate: true })}
          className={cn(
            'flex h-[22px] items-center gap-1 self-start rounded-full border px-2 text-[10.5px] transition-colors disabled:cursor-not-allowed',
            delegated
              ? 'border-acc-ring bg-acc-bg text-acc'
              : 'border-edge text-fg-3 hover:bg-shell-hover hover:text-fg-2',
          )}
        >
          {delegated ? <Check size={10} /> : <Sparkles size={10} />}
          交给你决定
        </button>
      )}
    </div>
  );
}

/** 选项卡:真按钮 + radio / checkbox 语义(同 WorkspacePicker 的游戏类型单选)。 */
function OptionCard({
  qid,
  opt,
  role,
  checked,
  blocked = false,
  blockedTitle,
  onClick,
}: {
  qid: string;
  opt: QuestionOption;
  role: 'radio' | 'checkbox';
  checked: boolean;
  /** multi 已选满:没选中的项不可再点。 */
  blocked?: boolean;
  blockedTitle?: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role={role}
      aria-checked={checked}
      disabled={blocked}
      title={blocked ? blockedTitle : undefined}
      data-testid={`q-${qid}-opt-${opt.id}`}
      onClick={onClick}
      className={cn(
        'flex w-full items-start gap-2 rounded-md border px-2 py-1.5 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-60',
        checked ? 'border-acc-ring bg-acc-bg' : 'border-edge bg-shell-panel hover:bg-shell-hover',
      )}
    >
      <span
        aria-hidden="true"
        className={cn(
          'mt-[3px] flex h-3 w-3 shrink-0 items-center justify-center border',
          role === 'radio' ? 'rounded-full' : 'rounded-[3px]',
          checked ? 'border-acc bg-acc text-fg-inv' : 'border-edge-strong',
        )}
      >
        {checked && <Check size={9} />}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex flex-wrap items-center gap-x-1.5 gap-y-0.5">
          <span className={cn('break-words text-[12px] leading-[17px]', checked ? 'text-acc' : 'text-fg-2')}>
            {opt.label}
          </span>
          {opt.recommended && (
            <span
              data-testid={`q-${qid}-opt-${opt.id}-recommended`}
              className="shrink-0 rounded-full bg-sage-bg px-1.5 text-[9.5px] leading-[15px] text-sage"
            >
              推荐
            </span>
          )}
        </span>
        {opt.description && (
          <span className="mt-0.5 block break-words text-[10.5px] leading-[15px] text-fg-4">{opt.description}</span>
        )}
      </span>
    </button>
  );
}
