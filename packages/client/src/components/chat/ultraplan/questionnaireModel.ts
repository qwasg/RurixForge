import type {
  QuestionKind,
  QuestionOption,
  Questionnaire,
  QuestionnaireQuestion,
  QuestionnaireSection,
  UltraPlanAnswer,
  UltraPlanAnswers,
} from '@/lib/ultraPlanStore';

/**
 * D-044:问卷卡的纯逻辑(契约 §2 Answers / §5 Questionnaire)。
 *
 * 问卷是模型写的 JSON(经后端校验后随 ultraplan.questionnaire 事件下发),答案是用户填的草稿;
 * 两头都可能残缺或过期(旧事件、localStorage 里上一版的草稿),所以这里一律先归一再用,
 * 组件里不再做防御判断。
 *
 * 「其他」的口径(契约没写死,如实留痕):
 * - single:选项与「其他」二选一,填了「其他」就不带 choice;
 * - multi:「其他」算一项,计入 min..max。
 */

const KINDS: readonly QuestionKind[] = ['single', 'multi', 'text', 'scale'];

/** scale 分段按钮的上限;范围再大就退成数字输入框(模型写出 0..1000 时不至于铺满一屏)。 */
export const SCALE_BUTTON_LIMIT = 21;

function asRecord(raw: unknown): Record<string, unknown> | null {
  return raw !== null && typeof raw === 'object' && !Array.isArray(raw)
    ? (raw as Record<string, unknown>)
    : null;
}

function str(raw: unknown): string {
  return typeof raw === 'string' ? raw : '';
}

function int(raw: unknown): number | undefined {
  return typeof raw === 'number' && Number.isInteger(raw) ? raw : undefined;
}

function normalizeOptions(raw: unknown): QuestionOption[] {
  if (!Array.isArray(raw)) return [];
  const seen = new Set<string>();
  const out: QuestionOption[] = [];
  for (const item of raw) {
    const rec = asRecord(item);
    const id = str(rec?.id);
    if (!rec || id === '' || seen.has(id)) continue;
    seen.add(id);
    const description = str(rec.description);
    out.push({
      id,
      label: str(rec.label) || id,
      ...(description !== '' ? { description } : {}),
      ...(rec.recommended === true ? { recommended: true } : {}),
    });
  }
  return out;
}

function normalizeQuestion(raw: unknown, seen: Set<string>): QuestionnaireQuestion | null {
  const rec = asRecord(raw);
  const id = str(rec?.id);
  if (!rec || id === '' || seen.has(id)) return null;
  const kind = KINDS.find((k) => k === rec.kind);
  if (!kind) return null;
  const options = kind === 'single' || kind === 'multi' ? normalizeOptions(rec.options) : [];
  const allowOther = (kind === 'single' || kind === 'multi') && rec.allowOther === true;
  // 选择题既没有选项也不许自填 → 没法作答,整题丢弃。
  if ((kind === 'single' || kind === 'multi') && options.length === 0 && !allowOther) return null;
  seen.add(id);
  const help = str(rec.help);
  const min = int(rec.min);
  const max = int(rec.max);
  const labels = Array.isArray(rec.scaleLabels) ? rec.scaleLabels : null;
  const scaleLabels: [string, string] | null =
    kind === 'scale' && labels && typeof labels[0] === 'string' && typeof labels[1] === 'string'
      ? [labels[0], labels[1]]
      : null;
  return {
    id,
    kind,
    question: str(rec.question) || id,
    ...(help !== '' ? { help } : {}),
    ...(options.length > 0 ? { options } : {}),
    ...(allowOther ? { allowOther: true } : {}),
    ...(typeof rec.allowDelegate === 'boolean' ? { allowDelegate: rec.allowDelegate } : {}),
    ...(typeof rec.required === 'boolean' ? { required: rec.required } : {}),
    ...(min !== undefined ? { min } : {}),
    ...(max !== undefined ? { max } : {}),
    ...(scaleLabels ? { scaleLabels } : {}),
  };
}

/** 事件载荷里的问卷 → 可渲染的问卷;不成形(不是对象)回 null。问题 id 重复时只留第一题。 */
export function normalizeQuestionnaire(raw: unknown): Questionnaire | null {
  const rec = asRecord(raw);
  if (!rec) return null;
  const seen = new Set<string>();
  const sections: QuestionnaireSection[] = [];
  const rawSections = Array.isArray(rec.sections) ? rec.sections : [];
  rawSections.forEach((item, index) => {
    const section = asRecord(item);
    if (!section) return;
    const questions = (Array.isArray(section.questions) ? section.questions : [])
      .map((q) => normalizeQuestion(q, seen))
      .filter((q): q is QuestionnaireQuestion => q !== null);
    if (questions.length === 0) return;
    sections.push({
      id: str(section.id) || `section-${index + 1}`,
      title: str(section.title) || `第 ${index + 1} 节`,
      questions,
    });
  });
  return { title: str(rec.title), understanding: str(rec.understanding), sections };
}

/** 缺省:single / multi / scale 必答,text 选答。 */
export function isRequired(q: QuestionnaireQuestion): boolean {
  return q.required ?? q.kind !== 'text';
}

/** 缺省允许「交给你决定」。 */
export function canDelegate(q: QuestionnaireQuestion): boolean {
  return q.allowDelegate !== false;
}

/** scale 取值范围(缺省 1..5;上下界写反时按缺省)。 */
export function scaleRange(q: QuestionnaireQuestion): { min: number; max: number } {
  const min = q.min ?? 1;
  const max = q.max ?? 5;
  return max >= min ? { min, max } : { min: 1, max: 5 };
}

/** multi 可选数量界:「其他」算一项;min 至少 1(选了才谈得上下界),且不超过 max。 */
export function multiBounds(q: QuestionnaireQuestion): { min: number; max: number } {
  const slots = (q.options?.length ?? 0) + (q.allowOther ? 1 : 0);
  const max = Math.min(Math.max(q.max ?? slots, 1), Math.max(slots, 1));
  const min = Math.min(Math.max(q.min ?? 1, 1), max);
  return { min, max };
}

function validChoice(q: QuestionnaireQuestion, answer: UltraPlanAnswer): string[] {
  const ids = new Set((q.options ?? []).map((o) => o.id));
  const out: string[] = [];
  for (const id of answer.choice ?? []) {
    if (ids.has(id) && !out.includes(id)) out.push(id);
  }
  return out;
}

function otherText(q: QuestionnaireQuestion, answer: UltraPlanAnswer): string {
  return q.allowOther && typeof answer.other === 'string' ? answer.other.trim() : '';
}

/** multi 已选项数(含「其他」)。 */
export function multiCount(q: QuestionnaireQuestion, answer: UltraPlanAnswer | undefined): number {
  if (!answer || answer.delegate === true) return 0;
  return validChoice(q, answer).length + (otherText(q, answer) !== '' ? 1 : 0);
}

/** ok = 已作答(含已委托);empty = 没填;invalid = 填了但不满足数量界。 */
export type AnswerStatus = 'ok' | 'empty' | 'invalid';

export function answerStatus(q: QuestionnaireQuestion, answer: UltraPlanAnswer | undefined): AnswerStatus {
  if (!answer) return 'empty';
  if (answer.delegate === true) return canDelegate(q) ? 'ok' : 'empty';
  switch (q.kind) {
    case 'single': {
      const picked = validChoice(q, answer).length;
      if (picked === 1) return 'ok';
      if (picked > 1) return 'invalid';
      return otherText(q, answer) !== '' ? 'ok' : 'empty';
    }
    case 'multi': {
      const count = multiCount(q, answer);
      if (count === 0) return 'empty';
      const { min, max } = multiBounds(q);
      return count < min || count > max ? 'invalid' : 'ok';
    }
    case 'text':
      return typeof answer.text === 'string' && answer.text.trim() !== '' ? 'ok' : 'empty';
    case 'scale': {
      const { min, max } = scaleRange(q);
      const value = answer.scale;
      return typeof value === 'number' && Number.isInteger(value) && value >= min && value <= max
        ? 'ok'
        : 'empty';
    }
    default:
      return 'empty';
  }
}

/** 这道题是否挡着提交:必答题没答好,或选答题填了一半(数量不够 / 超了)。 */
export function hasGap(q: QuestionnaireQuestion, answer: UltraPlanAnswer | undefined): boolean {
  const status = answerStatus(q, answer);
  return isRequired(q) ? status !== 'ok' : status === 'invalid';
}

/** 把一条原始答案收成契约形状(委托与具体答案互斥;空值不出字段);没有内容回 null。 */
function shapeAnswer(
  q: QuestionnaireQuestion,
  raw: unknown,
  trim: boolean,
): UltraPlanAnswer | null {
  const rec = asRecord(raw);
  if (!rec) return null;
  const answer = rec as UltraPlanAnswer;
  if (answer.delegate === true) return canDelegate(q) ? { delegate: true } : null;
  switch (q.kind) {
    case 'single': {
      const choice = validChoice(q, { choice: Array.isArray(answer.choice) ? answer.choice : [] });
      if (choice.length > 0) return { choice: [choice[0]] };
      const other = q.allowOther && typeof answer.other === 'string' ? answer.other : '';
      const text = trim ? other.trim() : other;
      return text.trim() !== '' ? { other: text } : null;
    }
    case 'multi': {
      const picked = validChoice(q, { choice: Array.isArray(answer.choice) ? answer.choice : [] });
      // 按问卷里的选项顺序出,不按点选顺序——同一组答案的请求体稳定。
      const choice = (q.options ?? []).map((o) => o.id).filter((id) => picked.includes(id));
      const other = q.allowOther && typeof answer.other === 'string' ? answer.other : '';
      const text = trim ? other.trim() : other;
      const hasOther = text.trim() !== '';
      if (choice.length === 0 && !hasOther) return null;
      return { ...(choice.length > 0 ? { choice } : {}), ...(hasOther ? { other: text } : {}) };
    }
    case 'text': {
      const value = typeof answer.text === 'string' ? answer.text : '';
      const text = trim ? value.trim() : value;
      return text.trim() !== '' ? { text } : null;
    }
    case 'scale':
      return answerStatus(q, { scale: answer.scale }) === 'ok' ? { scale: answer.scale } : null;
    default:
      return null;
  }
}

function allQuestions(questionnaire: Questionnaire): QuestionnaireQuestion[] {
  return questionnaire.sections.flatMap((s) => s.questions);
}

/** 草稿 → 只留这份问卷认得的题与选项(自由文本保留原样,不去首尾空白,继续打字不跳字)。 */
export function sanitizeDraft(questionnaire: Questionnaire, draft: unknown): UltraPlanAnswers {
  const rec = asRecord(draft);
  const out: UltraPlanAnswers = {};
  if (!rec) return out;
  for (const q of allQuestions(questionnaire)) {
    const answer = shapeAnswer(q, rec[q.id], false);
    if (answer) out[q.id] = answer;
  }
  return out;
}

/** 草稿 → 提交用的 Answers(契约 §2):文本去首尾空白,没答的选答题不出现在表里。 */
export function buildAnswers(questionnaire: Questionnaire, draft: UltraPlanAnswers): UltraPlanAnswers {
  const out: UltraPlanAnswers = {};
  for (const q of allQuestions(questionnaire)) {
    const answer = shapeAnswer(q, draft[q.id], true);
    if (answer && answerStatus(q, answer) === 'ok') out[q.id] = answer;
  }
  return out;
}

/** 还没答好、且允许委托的题。 */
export function delegatableRest(
  questionnaire: Questionnaire,
  draft: UltraPlanAnswers,
): QuestionnaireQuestion[] {
  return allQuestions(questionnaire).filter(
    (q) => canDelegate(q) && answerStatus(q, draft[q.id]) !== 'ok',
  );
}

/** 「其余全部交给你决定」:给每道没答好、且允许委托的题填 delegate;已答的不动。 */
export function delegateRest(questionnaire: Questionnaire, draft: UltraPlanAnswers): UltraPlanAnswers {
  const out: UltraPlanAnswers = { ...draft };
  for (const q of delegatableRest(questionnaire, draft)) out[q.id] = { delegate: true };
  return out;
}

export const DELEGATED_TEXT = '由 AI 决定';
export const UNANSWERED_TEXT = '未填';

/** 已提交答案的一行摘要:选项用 label(认不出的 id 原样显示),委托显示「由 AI 决定」。 */
export function answerSummary(q: QuestionnaireQuestion, raw: unknown): string {
  const answer = asRecord(raw) as UltraPlanAnswer | null;
  if (!answer) return UNANSWERED_TEXT;
  if (answer.delegate === true) return DELEGATED_TEXT;
  switch (q.kind) {
    case 'single':
    case 'multi': {
      const labels = (Array.isArray(answer.choice) ? answer.choice : [])
        .filter((id): id is string => typeof id === 'string')
        .map((id) => q.options?.find((o) => o.id === id)?.label ?? id);
      const other = typeof answer.other === 'string' ? answer.other.trim() : '';
      if (other !== '') labels.push(`其他:${other}`);
      return labels.length > 0 ? labels.join('、') : UNANSWERED_TEXT;
    }
    case 'text':
      return typeof answer.text === 'string' && answer.text.trim() !== ''
        ? answer.text.trim()
        : UNANSWERED_TEXT;
    case 'scale': {
      if (typeof answer.scale !== 'number') return UNANSWERED_TEXT;
      const { min, max } = scaleRange(q);
      return `${answer.scale}(${min}–${max})`;
    }
    default:
      return UNANSWERED_TEXT;
  }
}
