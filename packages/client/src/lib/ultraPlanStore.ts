import { create } from 'zustand';
import { ForgeApiError, apiGet } from './forgeApi';
import { usePlanStore } from './planStore';
import { useSessionStore } from './sessionStore';
import { useToastStore } from './toastStore';
import type { ChatBlock } from './timeline';

/**
 * D-044:UltraPlan 流程的前端状态面(wire 契约见 11_API_CONTRACTS E-11-007)。
 *
 * 流程本体是后端按会话持久化的阶段机(DebugSession.ultraplan),本 store 只是它的镜像:
 * - state 只有三个来源:快照 activeSession.ultraplan(hydrate)、GET …/ultraplan 重拉(refresh,
 *   由实时 ultraplan.* / session.updated 事件触发)、以及本 store 自己发出的 REST 动作的响应体。
 *   回放的历史事件**不**改这里(chatStore 回放只建卡片块)——否则重放会把阶段从头走一遍,
 *   重启过流程的会话会停在错的阶段上。
 * - 卡片动作(提交问卷 / 通过 Demo / 开始制作…)由 act() 自己 POST ask:execute,不经
 *   chatStore.sendMessage:动作没有用户正文(展示文案由后端写进 composer.user.message),
 *   且调用方要拿到「被拒绝」的结构化错误码。后端把整轮跑完才回 HTTP 响应,所以「受理」的
 *   信号不是 POST 返回,而是带同一 {id, action, rev} 的 composer.user.message 实时事件。
 * - 问卷草稿存 localStorage(切会话 / 刷新不丢),键 forge:ultraplan:draft:<id>:<rev>。
 */

// ---------- wire 类型(契约 §1 / §2 / §5 / §6) ----------

export const ULTRAPLAN_STAGES = [
  'discovery',
  'questionnaire',
  'demo_review',
  'plan_review',
  'production',
  'acceptance',
  'done',
] as const;

/** 流程正停在哪道关口(gate)。 */
export type UltraPlanStage = (typeof ULTRAPLAN_STAGES)[number];
export type UltraPlanPhase = 'waiting' | 'running' | 'failed';
/** 正在跑的 turn 种类(phase=running 时有值)。 */
export type UltraPlanTurnKind = 'discovery' | 'spec_demo' | 'planning' | 'production';

export interface UltraPlanError {
  code: string;
  message: string;
}

export interface UltraPlanState {
  id: string;
  /** demo-host 路径令牌(32 位小写 hex;不是 id / slug)。 */
  token: string;
  slug: string;
  /** ".forge/ultraplan/<slug>"(工作区根相对,正斜杠)。 */
  dir: string;
  title: string;
  workspaceId: string | null;
  stage: UltraPlanStage;
  phase: UltraPlanPhase;
  running: UltraPlanTurnKind | null;
  lastError: UltraPlanError | null;
  /** 0 = 还没出过问卷;之后 1,2,… */
  questionnaireRev: number;
  /** 0 = 还没有可玩的 Demo。 */
  demoIteration: number;
  demoVerified: boolean;
  demoNote: string | null;
  planPath: string | null;
  planRev: number;
  planHash: string | null;
  /** 最终游戏的渲染后端;与 Agent 执行引擎 local / codex 独立。 */
  renderBackend?: 'rurix' | 'godot';
  productionRunId: string | null;
  /** 0 = 还没到过验收;之后 1,2,… */
  acceptanceRound: number;
  createdAt: string;
  updatedAt: string;
}

/** 走 ask:execute 的 turn 动作。 */
export type UltraPlanAction =
  | 'answer'
  | 'approve_demo'
  | 'revise_demo'
  | 'revise_plan'
  | 'start_production'
  | 'resume_production'
  | 'fix_production';

/** 不起 turn 的 REST 动作(POST …/ultraplan/{action})。 */
export type UltraPlanRestAction = 'restart' | 'acceptance' | 'rollback_demo';

export interface UltraPlanAnswer {
  /** 选项 id(single 恰 1 个;multi 在 min..max 内)。 */
  choice?: string[];
  /** allowOther 时的自由填写。 */
  other?: string;
  /** kind=text。 */
  text?: string;
  /** kind=scale,[min,max] 内整数。 */
  scale?: number;
  /** 「交给你决定」;与其余字段互斥。 */
  delegate?: boolean;
}

export type UltraPlanAnswers = Record<string, UltraPlanAnswer>;

export type QuestionKind = 'single' | 'multi' | 'text' | 'scale';

export interface QuestionOption {
  id: string;
  label: string;
  description?: string;
  recommended?: boolean;
}

export interface QuestionnaireQuestion {
  /** 整份问卷内唯一。 */
  id: string;
  kind: QuestionKind;
  question: string;
  help?: string;
  /** single / multi:2..6 项。 */
  options?: QuestionOption[];
  /** 缺省 false。 */
  allowOther?: boolean;
  /** 缺省 true(「交给你决定」)。 */
  allowDelegate?: boolean;
  /** 缺省:single / multi / scale 为 true,text 为 false。 */
  required?: boolean;
  /** multi:可选数量界;scale:取值范围(缺省 1..5)。 */
  min?: number;
  max?: number;
  scaleLabels?: [string, string];
}

export interface QuestionnaireSection {
  id: string;
  title: string;
  questions: QuestionnaireQuestion[];
}

export interface Questionnaire {
  title: string;
  /** markdown:对设想的理解、假设、项目调研结论。 */
  understanding: string;
  sections: QuestionnaireSection[];
}

export interface AutomatedCheck {
  id: string;
  kind: 'visual' | 'gameplay';
  scene?: string;
  steps: string;
  expected: string;
}

export interface ManualCheck {
  id: string;
  title: string;
  steps: string;
  expected: string;
  /** 缺省 true;必要检查点不可跳过。 */
  required?: boolean;
}

export interface UltraPlanChecks {
  automated: AutomatedCheck[];
  manual: ManualCheck[];
}

/** Demo 宿主坐标;URL 由 demoUrl() 在前端拼(事件里从不带绝对 URL,端口每次重启都变)。 */
export interface UltraPlanDemo {
  port: number;
  token: string;
  entry: string;
}

export interface UltraPlanDelivery {
  entry: string;
  controls: string;
}

export function normalizeDelivery(raw: unknown): UltraPlanDelivery | null {
  const value = asRecord(raw);
  if (!value || typeof value.entry !== 'string' || !value.entry.trim() || typeof value.controls !== 'string') return null;
  return { entry: value.entry, controls: value.controls };
}

export type AcceptanceStatus = 'pass' | 'fail' | 'skip';

export interface AcceptanceResult {
  id: string;
  status: AcceptanceStatus;
  /** status=fail 或可选项 skip 时必填。 */
  note?: string;
}

/** acceptance.json 的一轮。契约只定了外壳 { rounds: [...] },字段按 acceptance.recorded 载荷取宽。 */
export interface UltraPlanAcceptanceRound {
  round?: number;
  results?: AcceptanceResult[];
  failed?: string[];
  [key: string]: unknown;
}

export interface UltraPlanAcceptance {
  rounds: UltraPlanAcceptanceRound[];
}

/** composer.user.message.payload.ultraplan(动作回显;resume_production 无 rev)。 */
export interface UltraPlanActionRef {
  id: string;
  action: string;
  rev: number | null;
}

/** chatStore 转交的实时事件(只取用得到的三项)。 */
export interface UltraPlanEventLike {
  sessionId?: string;
  type: string;
  payload?: Record<string, unknown>;
}

/** ultraplan.notice(仅实时;如 THINKING_UNAVAILABLE / EXPLORE_SKIPPED)。 */
export interface UltraPlanNotice {
  /** 流程 id。 */
  id: string;
  runId: string | null;
  code: string;
  message: string;
}

// ---------- 动作结果 ----------

export interface ActFailure {
  ok: false;
  code: string;
  message: string;
  /** 后端 error.details 原文(如 STAGE_MISMATCH 的 {stage, allowed});无则 null。 */
  details: Record<string, unknown> | null;
}

export type ActResult = { ok: true } | ActFailure;

export type AcceptanceOutcome =
  | { ok: true; next: 'done' | 'fix'; failed: string[] }
  | ActFailure;

/** 动作落在哪张卡上:缺省取当前 state;卡片 / 页签应传自己那一版(跨会话、过期卡由后端 409)。 */
export interface UltraPlanTarget {
  id?: string;
  rev?: number;
}

export interface ActOptions extends UltraPlanTarget {
  /** revise_demo / revise_plan 的修改意见(必填);其余动作留空,展示文案由后端写。 */
  userInput?: string;
  /** 仅 answer;重试时可省(后端复用该 rev 的 answers.json)。 */
  answers?: UltraPlanAnswers;
  /** auto 权限下确认「制作期间逐项审批」;仅三个制作动作。 */
  acknowledgeApprovals?: boolean;
  planPath?: string;
}

/** 前端本地就能判定、没有发请求的失败(不是后端错误码)。 */
export const CLIENT_ERROR = {
  noSession: 'CLIENT_NO_SESSION',
  noFlow: 'CLIENT_NO_FLOW',
  pending: 'CLIENT_ACTION_PENDING',
  /** 动作在途时切了会话:请求已发出,它的成败回原会话看,这边不再等。 */
  sessionChanged: 'CLIENT_SESSION_CHANGED',
  invalidInput: 'INVALID_INPUT',
} as const;

// ---------- 纯函数(组件共用) ----------

type UltraBlock = Extract<ChatBlock, { kind: 'ultraplan' }>;
export type UltraPlanStep = UltraBlock['step'];

/** 卡片 step ↔ 流程关口。 */
const STEP_STAGE: Record<UltraPlanStep, UltraPlanStage> = {
  questionnaire: 'questionnaire',
  demo: 'demo_review',
  plan: 'plan_review',
  acceptance: 'acceptance',
  done: 'done',
};

const STAGE_LABELS: Record<UltraPlanStage, string> = {
  discovery: '需求',
  questionnaire: '问卷',
  demo_review: 'Demo',
  plan_review: '计划',
  production: '制作',
  acceptance: '验收',
  done: '完成',
};

/** 阶段序号(0 起;未知阶段 -1)。 */
export function stageIndex(stage: string | null | undefined): number {
  return ULTRAPLAN_STAGES.indexOf(stage as UltraPlanStage);
}

/** 阶段短名:需求 / 问卷 / Demo / 计划 / 制作 / 验收 / 完成(未知阶段回空串)。 */
export function stageLabel(stage: string | null | undefined): string {
  return STAGE_LABELS[stage as UltraPlanStage] ?? '';
}

/** 卡片 step 对应的 state 版本号(done 无版本 → null)。 */
export function stateRevFor(step: UltraPlanStep, state: UltraPlanState): number | null {
  switch (step) {
    case 'questionnaire':
      return state.questionnaireRev;
    case 'demo':
      return state.demoIteration;
    case 'plan':
      return state.planRev;
    case 'acceptance':
      return state.acceptanceRound;
    default:
      return null;
  }
}

/**
 * 卡片是否可操作(契约 §9):同一流程 && 关口对得上 step && 版本是当前版 && 没在跑 &&
 * 没有活动 run && 还没提交过。只看阶段不够——同一关口下问卷可以重出、Demo 有多轮,
 * 旧卡必须只读。done 卡没有版本号,不比 rev。
 */
export function cardInteractive(
  block: UltraBlock,
  state: UltraPlanState | null,
  activeRunId: string | null,
): boolean {
  if (!state) return false;
  if (block.upId !== state.id) return false;
  if (STEP_STAGE[block.step] !== state.stage) return false;
  const rev = stateRevFor(block.step, state);
  if (rev !== null && block.rev !== rev) return false;
  if (state.phase === 'running') return false;
  if (activeRunId !== null) return false;
  return !block.submitted;
}

const RUNNING_TEXT: Record<UltraPlanTurnKind, string> = {
  discovery: '正在理解需求并起草问卷…',
  spec_demo: '正在构建 Demo…',
  planning: '正在编写制作计划…',
  production: '正在制作…',
};

const FAILED_TEXT: Record<UltraPlanTurnKind, string> = {
  discovery: '需求分析中断',
  spec_demo: 'Demo 构建中断',
  planning: '计划编写中断',
  production: '制作中断',
};

/** 失败码 → 是哪一种 turn 断了(失败后 running 已清空时用)。 */
const FAILED_CODE_KIND: Record<string, UltraPlanTurnKind> = {
  ULTRAPLAN_SPEC_MISSING: 'spec_demo',
  ULTRAPLAN_DEMO_BUILD_FAILED: 'spec_demo',
  ULTRAPLAN_DEMO_MISSING: 'spec_demo',
  ULTRAPLAN_FIX_NO_TASKS: 'production',
  ULTRAPLAN_PRODUCTION_INCOMPLETE: 'production',
};

/** 失败态下断的是哪种 turn:running 还留着就用它,否则按失败码推;推不出回 null。 */
export function failedTurnKind(state: UltraPlanState): UltraPlanTurnKind | null {
  return state.running ?? FAILED_CODE_KIND[state.lastError?.code ?? ''] ?? null;
}

const WAITING_TEXT: Record<UltraPlanStage, string> = {
  discovery: '等待你补充需求',
  questionnaire: '等待你填写问卷',
  demo_review: '等待你试玩 Demo',
  plan_review: '等待你确认计划',
  production: '等待继续制作',
  acceptance: '等待你验收',
  done: '已完成',
};

/** 状态条的一句话:正在做什么 / 在等什么 / 断在哪(无流程回空串)。 */
export function phaseText(state: UltraPlanState | null): string {
  if (!state) return '';
  if (state.phase === 'running') {
    return state.running ? RUNNING_TEXT[state.running] ?? '正在处理…' : '正在处理…';
  }
  if (state.phase === 'failed') {
    const kind = failedTurnKind(state);
    if (kind && FAILED_TEXT[kind]) return FAILED_TEXT[kind];
    if (state.stage === 'production' || state.stage === 'acceptance') return FAILED_TEXT.production;
    return '上一步未完成';
  }
  return WAITING_TEXT[state.stage] ?? '';
}

/**
 * 阶段机说 Planning turn 正在跑(写计划 / 改计划)。Plan 页签的「正在调研…」条除了
 * act() 发起时点亮,还要认它——刷新 / 重连 / 别的窗口发起的那一轮,本页只能从阶段机得知。
 */
export function isPlanningTurn(state: UltraPlanState | null): boolean {
  return state !== null && state.phase === 'running' && state.running === 'planning';
}

const LOOPBACK_HOSTS = new Set(['127.0.0.1', 'localhost', '[::1]']);

export function isLoopbackHost(hostname: string): boolean {
  return LOOPBACK_HOSTS.has(hostname.toLowerCase());
}

/**
 * 赋给 iframe.src 前的四条断言(契约 §8):http 协议、回环主机名、与应用不同源、
 * 路径落在 /u/<token>/ 之下。iframe 带 allow-same-origin,一旦指到应用自己的源,
 * 沙箱就等于没有——所以任何一条不成立都不加载。
 */
export function demoUrlAllowed(url: string, token: string, locationOrigin: string): boolean {
  if (token === '') return false;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return false;
  }
  if (parsed.protocol !== 'http:') return false;
  if (parsed.username !== '' || parsed.password !== '') return false;
  if (!isLoopbackHost(parsed.hostname)) return false;
  if (parsed.origin === locationOrigin) return false;
  return parsed.pathname.startsWith(`/u/${token}/`);
}

/**
 * Demo 地址:http://<host>:<port>/u/<token>/?v=<iteration>。
 * host 取与应用**不同**的那个回环名(应用在 127.0.0.1 → localhost,否则 127.0.0.1):
 * 不同主机名即跨站,Demo 进独立渲染进程,跑飞的 Demo 卡不死 IDE。
 * 任何一条断言不成立(或入参不成形)→ null,调用方显示错误态,绝不把可疑地址塞进 iframe。
 */
export function demoUrl(
  demo: UltraPlanDemo | null | undefined,
  locationHostname: string,
  iteration: number,
  locationOrigin: string = typeof location === 'undefined' ? '' : location.origin,
): string | null {
  if (!demo) return null;
  const { port, token } = demo;
  if (typeof token !== 'string' || token === '') return null;
  if (!Number.isInteger(port) || port < 1 || port > 65535) return null;
  if (!Number.isInteger(iteration) || iteration < 0) return null;
  const host = locationHostname === '127.0.0.1' ? 'localhost' : '127.0.0.1';
  const url = `http://${host}:${String(port)}/u/${token}/`;
  if (!demoUrlAllowed(url, token, locationOrigin)) return null;
  const parsed = new URL(url);
  // token / port 里夹带 ?、# 会让断言过的路径与实际请求的不是一回事。
  if (parsed.search !== '' || parsed.hash !== '') return null;
  if (parsed.port !== String(port)) return null;
  parsed.search = `?v=${iteration}`;
  return parsed.toString();
}

// ---------- 问卷 / 验收清单草稿(localStorage;不可用时静默退化为不持久) ----------

function draftStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null; // 隐私模式 / 被策略禁用时访问即抛
  }
}

/** 读一个 JSON 对象草稿;不存在 / 损坏 / 不是对象 → null。 */
function readDraftObject(key: string): Record<string, unknown> | null {
  const storage = draftStorage();
  if (!storage) return null;
  try {
    const raw = storage.getItem(key);
    if (raw === null) return null;
    const value: unknown = JSON.parse(raw);
    return value !== null && typeof value === 'object' && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function writeDraft(key: string, value: unknown): void {
  const storage = draftStorage();
  if (!storage) return;
  try {
    storage.setItem(key, JSON.stringify(value));
  } catch {
    // 配额满等:草稿只是便利,丢了不报错。
  }
}

function removeDraft(key: string): void {
  const storage = draftStorage();
  if (!storage) return;
  try {
    storage.removeItem(key);
  } catch {
    // 同上。
  }
}

export function draftKey(id: string, rev: number): string {
  return `forge:ultraplan:draft:${id}:${rev}`;
}

export function loadDraft(id: string, rev: number): UltraPlanAnswers | null {
  return readDraftObject(draftKey(id, rev)) as UltraPlanAnswers | null;
}

export function saveDraft(id: string, rev: number, answers: UltraPlanAnswers): void {
  writeDraft(draftKey(id, rev), answers);
}

export function clearDraft(id: string, rev: number): void {
  removeDraft(draftKey(id, rev));
}

/** 验收清单草稿:检查点 id → 已选的结论 + 说明(都可缺)。 */
export type AcceptanceDraft = Record<string, { status?: AcceptanceStatus; note?: string }>;

const ACCEPTANCE_STATUSES: ReadonlySet<string> = new Set<AcceptanceStatus>(['pass', 'fail', 'skip']);

export function acceptanceDraftKey(id: string, round: number): string {
  return `forge:ultraplan:acceptance:${id}:${round}`;
}

/** 读验收清单草稿;逐项按「可能被改坏」读,不成形的条目丢掉(不抛)。 */
export function loadAcceptanceDraft(id: string, round: number): AcceptanceDraft | null {
  const raw = readDraftObject(acceptanceDraftKey(id, round));
  if (!raw) return null;
  const draft: AcceptanceDraft = {};
  for (const [checkId, value] of Object.entries(raw)) {
    const rec = asRecord(value);
    if (!rec) continue;
    const status =
      typeof rec.status === 'string' && ACCEPTANCE_STATUSES.has(rec.status)
        ? (rec.status as AcceptanceStatus)
        : undefined;
    const note = typeof rec.note === 'string' ? rec.note : undefined;
    if (status === undefined && note === undefined) continue;
    draft[checkId] = { ...(status ? { status } : {}), ...(note !== undefined ? { note } : {}) };
  }
  return draft;
}

export function saveAcceptanceDraft(id: string, round: number, draft: AcceptanceDraft): void {
  writeDraft(acceptanceDraftKey(id, round), draft);
}

export function clearAcceptanceDraft(id: string, round: number): void {
  removeDraft(acceptanceDraftKey(id, round));
}

// ---------- 归一(后端字段缺失 / 旧版本时不让 undefined 漏进比较) ----------

function asRecord(raw: unknown): Record<string, unknown> | null {
  return raw !== null && typeof raw === 'object' && !Array.isArray(raw)
    ? (raw as Record<string, unknown>)
    : null;
}

function normalizeError(raw: unknown): UltraPlanError | null {
  const rec = asRecord(raw);
  if (!rec || typeof rec.code !== 'string') return null;
  return { code: rec.code, message: typeof rec.message === 'string' ? rec.message : '' };
}

export function normalizeUltraPlanState(raw: unknown): UltraPlanState | null {
  const rec = asRecord(raw);
  if (!rec || typeof rec.id !== 'string' || rec.id === '') return null;
  const str = (key: string): string => (typeof rec[key] === 'string' ? (rec[key] as string) : '');
  const strOrNull = (key: string): string | null =>
    typeof rec[key] === 'string' ? (rec[key] as string) : null;
  const num = (key: string): number =>
    typeof rec[key] === 'number' && Number.isFinite(rec[key]) ? (rec[key] as number) : 0;
  const stage = ULTRAPLAN_STAGES.includes(rec.stage as UltraPlanStage)
    ? (rec.stage as UltraPlanStage)
    : 'discovery';
  const phase: UltraPlanPhase =
    rec.phase === 'running' || rec.phase === 'failed' ? rec.phase : 'waiting';
  const running: UltraPlanTurnKind | null =
    rec.running === 'discovery' ||
    rec.running === 'spec_demo' ||
    rec.running === 'planning' ||
    rec.running === 'production'
      ? rec.running
      : null;
  return {
    id: rec.id,
    token: str('token'),
    slug: str('slug'),
    dir: str('dir'),
    title: str('title'),
    workspaceId: strOrNull('workspaceId'),
    stage,
    phase,
    running,
    lastError: normalizeError(rec.lastError),
    questionnaireRev: num('questionnaireRev'),
    demoIteration: num('demoIteration'),
    demoVerified: rec.demoVerified === true,
    demoNote: strOrNull('demoNote'),
    planPath: strOrNull('planPath'),
    planRev: num('planRev'),
    planHash: strOrNull('planHash'),
    ...(rec.renderBackend === 'rurix' || rec.renderBackend === 'godot' ? { renderBackend: rec.renderBackend } : {}),
    productionRunId: strOrNull('productionRunId'),
    acceptanceRound: num('acceptanceRound'),
    createdAt: str('createdAt'),
    updatedAt: str('updatedAt'),
  };
}

function normalizeDemo(raw: unknown): UltraPlanDemo | null {
  const rec = asRecord(raw);
  if (!rec || typeof rec.port !== 'number' || typeof rec.token !== 'string') return null;
  return {
    port: rec.port,
    token: rec.token,
    entry: typeof rec.entry === 'string' ? rec.entry : 'index.html',
  };
}

// ---------- 请求 ----------

/** GET …/ultraplan 响应面(契约 §3)。 */
interface UltraPlanFace {
  delivery?: unknown;
  ultraplan?: unknown;
  demo?: unknown;
  demoError?: unknown;
  questionnaire?: unknown;
  answers?: unknown;
  checks?: unknown;
  acceptance?: unknown;
  renderBackend?: unknown;
}

type PostResult<T> = { ok: true; body: T } | (ActFailure & { status: number });

/**
 * 自带的 POST:forgeApi.apiPost 抛出的 ForgeApiError 只有 code / message,装不下
 * error.details(STAGE_MISMATCH 的 {stage, allowed}、ANSWERS_INVALID 的 {questionId}),
 * 卡片要靠它定位,故这里自行解析错误体。payload 缺省 = 无请求体(restart / rollback_demo)。
 */
async function postJson<T>(path: string, payload?: unknown): Promise<PostResult<T>> {
  let res: Response;
  try {
    res = await fetch(
      path,
      payload === undefined
        ? { method: 'POST' }
        : {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(payload),
          },
    );
  } catch (err) {
    return {
      ok: false,
      status: 0,
      code: 'NETWORK',
      message: `请求失败: ${err instanceof Error ? err.message : String(err)}`,
      details: null,
    };
  }
  let body: unknown = null;
  try {
    body = await res.json();
  } catch {
    body = null;
  }
  if (!res.ok) {
    const err = asRecord(asRecord(body)?.error);
    return {
      ok: false,
      status: res.status,
      code: typeof err?.code === 'string' ? err.code : `HTTP_${res.status}`,
      message: typeof err?.message === 'string' ? err.message : `HTTP ${res.status}`,
      details: asRecord(err?.details),
    };
  }
  return { ok: true, body: body as T };
}

function activeSessionId(): string | null {
  return useSessionStore.getState().activeSessionId;
}

function sessionPath(sessionId: string, tail: string): string {
  return `/api/forge/sessions/${encodeURIComponent(sessionId)}/${tail}`;
}

/** fetchDemoFace 的结果:某会话的流程状态 + Demo 宿主坐标(与 store 同一套归一)。 */
export interface DemoFace {
  state: UltraPlanState | null;
  demo: UltraPlanDemo | null;
  demoError: UltraPlanError | null;
  /**
   * GET …/ultraplan 本身失败的原因(引擎离线 502 / 会话已删 404 / 旧后端无此路由…);成功为 null。
   * 失败时 state / demo 是取不到,不是「没有」——调用方必须先报它,不能当成「还没有 Demo」(I-5)。
   */
  faceError?: UltraPlanError | null;
}

/** GET …/ultraplan 失败 → 结构化原因:后端错误码原样;连不上 / 回包不是 JSON 各给一个码。 */
export function faceErrorOf(err: unknown): UltraPlanError {
  if (err instanceof ForgeApiError) {
    return { code: err.code, message: err.message };
  }
  if (err instanceof TypeError) {
    return { code: 'NETWORK', message: `连不上引擎:${err.message}` };
  }
  return {
    code: 'DEMO_INFO_UNAVAILABLE',
    message: `取不到流程信息:${err instanceof Error ? err.message : String(err)}`,
  };
}

/**
 * 只读取回任一会话的 GET …/ultraplan,**不落 store**。store 只镜像当前会话;Demo 页签是全局的,
 * 切到别的会话后仍要能看原会话的 Demo(动作禁用),坐标就走这里单独取。
 * 失败(离线 / 会话已删 / 旧后端)不吞:原因放 faceError,state / demo 为 null。
 */
export async function fetchDemoFace(sessionId: string): Promise<DemoFace> {
  let face: UltraPlanFace;
  try {
    face = await apiGet<UltraPlanFace>(sessionPath(sessionId, 'ultraplan'));
  } catch (err) {
    return { state: null, demo: null, demoError: null, faceError: faceErrorOf(err) };
  }
  const state = normalizeUltraPlanState(face?.ultraplan);
  return {
    state,
    demo: state ? normalizeDemo(face.demo) : null,
    demoError: state ? normalizeError(face.demoError) : null,
    faceError: null,
  };
}

/** 给用户看的失败文案:大多直接用后端 message;个别码后端文案不对场景,换一句。 */
export function actionErrorText(code: string, message: string): string {
  if (code === 'SESSION_BUSY') return '有任务正在运行,请等它结束后再操作';
  if (code === CLIENT_ERROR.pending) return '上一个操作还在提交中';
  return message.trim() === '' ? code : message;
}

function failure(
  code: string,
  message: string,
  details: Record<string, unknown> | null = null,
): ActFailure {
  return { ok: false, code, message, details };
}

/** 三个制作动作走 team 模式,其余走 ultraplan(契约 §2 表)。 */
const PRODUCTION_ACTIONS: ReadonlySet<UltraPlanAction> = new Set<UltraPlanAction>([
  'start_production',
  'resume_production',
  'fix_production',
]);

/** 动作要带的版本号(resume_production 不带 → null)。 */
function actionRev(action: UltraPlanAction, state: UltraPlanState): number | null {
  switch (action) {
    case 'answer':
      return state.questionnaireRev;
    case 'approve_demo':
    case 'revise_demo':
      return state.demoIteration;
    case 'revise_plan':
    case 'start_production':
      return state.planRev;
    case 'fix_production':
      return state.acceptanceRound;
    default:
      return null;
  }
}

// ---------- store ----------

interface UltraPlanStoreState {
  /** state 属于哪个会话(全局页签据此判断「这是另一个会话的 Demo / 计划」)。 */
  sessionId: string | null;
  state: UltraPlanState | null;
  demo: UltraPlanDemo | null;
  /** Demo 监听起不来(DEMO_HOST_UNAVAILABLE);此时 demo 为 null。 */
  demoError: UltraPlanError | null;
  /**
   * 最近一次 GET …/ultraplan 失败的原因(离线 / 502 / 404 / 旧后端);成功即清。失败时 state 仍是
   * 快照回填的那份、附属数据(demo 等)取不到——Demo 页签据此报真实原因,不误报「没有 Demo」(I-5)。
   */
  faceError: UltraPlanError | null;
  questionnaire: Questionnaire | null;
  answers: UltraPlanAnswers | null;
  checks: UltraPlanChecks | null;
  delivery: UltraPlanDelivery | null;
  acceptance: UltraPlanAcceptance | null;
  /** 在途动作:请求已发、还没见到受理事件也没被拒。期间所有卡片 / 状态条动作应禁用。 */
  pending: { action: UltraPlanAction | UltraPlanRestAction; rev: number | null } | null;
  /** 最近一次被拒的动作(下一次动作发起时清空)。 */
  lastActionError: (UltraPlanError & { details?: Record<string, unknown> | null }) | null;
  /** 当前 turn 实际生效的思考规格(仅实时 ultraplan.stage 带;刷新后为 null)。 */
  live: { effort: string | null; thinkingForced: boolean } | null;
  /** 实时 ultraplan.notice(仅本次页面存活期;上限 8 条)。 */
  notices: UltraPlanNotice[];

  /** 快照回填(sessionId 缺省取当前会话)。 */
  hydrate: (ultraplan: unknown, sessionId?: string | null) => void;
  /** GET …/ultraplan;同会话的并发调用合并(在途时再来的请求折成结束后补拉一次)。 */
  refresh: (sessionId?: string | null) => Promise<void>;
  act: (action: UltraPlanAction, opts?: ActOptions) => Promise<ActResult>;
  /** answers 省略 = 重试(后端复用已存答案)。 */
  submitAnswers: (answers?: UltraPlanAnswers, target?: UltraPlanTarget) => Promise<ActResult>;
  approveDemo: (target?: UltraPlanTarget) => Promise<ActResult>;
  reviseDemo: (feedback: string, target?: UltraPlanTarget) => Promise<ActResult>;
  revisePlan: (feedback: string, target?: UltraPlanTarget) => Promise<ActResult>;
  startProduction: (acknowledge?: boolean, target?: UltraPlanTarget) => Promise<ActResult>;
  resumeProduction: (acknowledge?: boolean) => Promise<ActResult>;
  fixProduction: (acknowledge?: boolean, target?: UltraPlanTarget) => Promise<ActResult>;
  /** 清掉流程状态(产物文件保留)。 */
  restart: () => Promise<ActResult>;
  /** 提交人工验收;next=fix 时由调用方接着发 fixProduction()。 */
  submitAcceptance: (results: AcceptanceResult[], round?: number) => Promise<AcceptanceOutcome>;
  rollbackDemo: () => Promise<ActResult>;
  /** chatStore:实时 composer.user.message 带 ultraplan 回显 → 判定在途动作已被受理。 */
  noteUserMessage: (ref: UltraPlanActionRef) => void;
  /** chatStore:实时 ultraplan.* 事件 → 记 live / notice 并触发 refresh(回放事件不进这里)。 */
  applyLiveEvent: (event: UltraPlanEventLike) => void;
  reset: () => void;
}

const MAX_NOTICES = 8;

/** reset() 递增:在途请求回来时若代次已变(切了会话),结果一律不落 store。 */
let epoch = 0;

interface RefreshSlot {
  sessionId: string;
  again: boolean;
  promise: Promise<void>;
}
let refreshSlot: RefreshSlot | null = null;

interface ActSlot {
  id: string;
  action: UltraPlanAction;
  rev: number | null;
  planning: boolean;
  settle: (result: ActResult) => void;
}
let actSlot: ActSlot | null = null;

const EMPTY = {
  sessionId: null,
  state: null,
  demo: null,
  demoError: null,
  faceError: null,
  questionnaire: null,
  answers: null,
  checks: null,
  delivery: null,
  acceptance: null,
  pending: null,
  lastActionError: null,
  live: null,
  notices: [],
} satisfies Partial<UltraPlanStoreState>;

export const useUltraPlanStore = create<UltraPlanStoreState>((set, get) => {
  /** 被拒:清在途、记错误、toast(warning,不用 danger 红)、重拉真实阶段。 */
  const reject = (sessionId: string, fail: ActFailure): void => {
    set({
      pending: null,
      lastActionError: { code: fail.code, message: fail.message, details: fail.details },
    });
    useToastStore.getState().push('warning', actionErrorText(fail.code, fail.message));
    void get().refresh(sessionId);
  };

  /** 受理:清在途;问卷答案已入库,本地草稿随之作废。 */
  const accept = (slot: ActSlot): void => {
    set({ pending: null });
    if (slot.action === 'answer' && slot.rev !== null) clearDraft(slot.id, slot.rev);
  };

  const rest = async <T>(
    action: UltraPlanRestAction,
    payload?: unknown,
  ): Promise<{ ok: true; body: T; sessionId: string; stale: boolean } | ActFailure> => {
    const sessionId = activeSessionId();
    if (!sessionId) return failure(CLIENT_ERROR.noSession, '请先选择会话');
    if (get().pending) return failure(CLIENT_ERROR.pending, '上一个操作还在提交中');
    const started = epoch;
    set({ pending: { action, rev: null }, lastActionError: null });
    const result = await postJson<T>(sessionPath(sessionId, `ultraplan/${action}`), payload);
    const stale = started !== epoch || activeSessionId() !== sessionId;
    if (!result.ok) {
      const fail = failure(result.code, result.message, result.details);
      if (!stale) reject(sessionId, fail);
      return fail;
    }
    if (!stale) set({ pending: null });
    return { ok: true, body: result.body, sessionId, stale };
  };

  return {
    ...EMPTY,

    hydrate: (ultraplan, sessionId) => {
      const sid = sessionId === undefined ? activeSessionId() : sessionId;
      const next = normalizeUltraPlanState(ultraplan);
      const prev = get();
      // 同一流程的重复回填(gap 重拉快照)保留问卷 / Demo 坐标,等 refresh 覆盖;
      // 换了流程或没有流程 → 旧流程的附属数据一并清掉,不让它挂在新状态上。
      const sameFlow = next !== null && prev.state?.id === next.id && prev.sessionId === sid;
      set(
        sameFlow
          ? { sessionId: sid, state: next }
          : {
              sessionId: sid,
              state: next,
              demo: null,
              demoError: null,
              faceError: null,
              questionnaire: null,
              answers: null,
              checks: null,
              delivery: null,
              acceptance: null,
            },
      );
    },

    refresh: (requested) => {
      const sessionId = requested === undefined ? activeSessionId() : requested;
      if (!sessionId) return Promise.resolve();
      if (refreshSlot && refreshSlot.sessionId === sessionId) {
        refreshSlot.again = true;
        return refreshSlot.promise;
      }
      const started = epoch;
      const slot: RefreshSlot = { sessionId, again: false, promise: Promise.resolve() };
      refreshSlot = slot;
      const run = async (): Promise<void> => {
        try {
          do {
            slot.again = false;
            let face: UltraPlanFace;
            try {
              face = await apiGet<UltraPlanFace>(sessionPath(sessionId, 'ultraplan'));
            } catch (err) {
              // 离线 / 旧后端无此路由:保留快照回填的状态(离线态由 StatusBar 呈现),但记下失败原因——
              // 附属数据此刻是「取不到」不是「没有」,Demo 页签要据此如实报错。
              if (started === epoch && activeSessionId() === sessionId) {
                set({ faceError: faceErrorOf(err) });
              }
              return;
            }
            if (started !== epoch || activeSessionId() !== sessionId) return;
            const next = normalizeUltraPlanState(face?.ultraplan);
            if (next && (face.renderBackend === 'rurix' || face.renderBackend === 'godot')) {
              next.renderBackend = face.renderBackend;
            }
            set({
              sessionId,
              state: next,
              demo: next ? normalizeDemo(face.demo) : null,
              demoError: next ? normalizeError(face.demoError) : null,
              faceError: null,
              questionnaire: next ? (asRecord(face.questionnaire) as Questionnaire | null) : null,
              answers: next ? (asRecord(face.answers) as UltraPlanAnswers | null) : null,
              checks: next ? (asRecord(face.checks) as UltraPlanChecks | null) : null,
              delivery: next ? normalizeDelivery(face.delivery) : null,
              acceptance: next ? (asRecord(face.acceptance) as UltraPlanAcceptance | null) : null,
            });
          } while (slot.again);
        } finally {
          if (refreshSlot === slot) refreshSlot = null;
        }
      };
      slot.promise = run();
      return slot.promise;
    },

    act: (action, opts = {}) => {
      const sessionId = activeSessionId();
      const state = get().state;
      if (!sessionId) return Promise.resolve(failure(CLIENT_ERROR.noSession, '请先选择会话'));
      if (!state) {
        return Promise.resolve(failure(CLIENT_ERROR.noFlow, '当前会话没有进行中的 UltraPlan 流程'));
      }
      if (get().pending) {
        return Promise.resolve(failure(CLIENT_ERROR.pending, '上一个操作还在提交中'));
      }
      const userInput = (opts.userInput ?? '').trim();
      if ((action === 'revise_demo' || action === 'revise_plan') && userInput === '') {
        return Promise.resolve(failure(CLIENT_ERROR.invalidInput, '请先填写修改意见'));
      }
      const id = opts.id ?? state.id;
      const rev = action === 'resume_production' ? null : (opts.rev ?? actionRev(action, state));
      const body = {
        userInput,
        mode: PRODUCTION_ACTIONS.has(action) ? 'team' : 'ultraplan',
        ultraplan: {
          id,
          ...(rev !== null ? { rev } : {}),
          action,
          ...(opts.answers !== undefined ? { answers: opts.answers } : {}),
          ...(opts.acknowledgeApprovals ? { acknowledgeApprovals: true } : {}),
        },
        ...(opts.planPath !== undefined ? { planPath: opts.planPath } : {}),
      };
      // 通过 Demo / 要求改计划 → 后端起 Planning turn:Plan 页签的「正在调研…」条随之亮起
      // (终态由 chatStore 的 run 收束事件复位,与 plan 模式同一套)。
      const planning = action === 'approve_demo' || action === 'revise_plan';
      const started = epoch;

      return new Promise<ActResult>((resolve) => {
        let settled = false;
        const slot: ActSlot = {
          id,
          action,
          rev,
          planning,
          settle: (result) => {
            if (settled) return;
            settled = true;
            resolve(result);
          },
        };
        actSlot = slot;
        set({ pending: { action, rev }, lastActionError: null });
        if (planning) usePlanStore.getState().setPlanning(true);

        void postJson<unknown>(sessionPath(sessionId, 'ask:execute'), body).then((result) => {
          // current = 还没被受理事件结掉,也没被 reset() 作废。
          const current = actSlot === slot && started === epoch;
          if (actSlot === slot) actSlot = null;
          if (result.ok) {
            // 整轮已收束(HTTP 响应在 turn 结束时才回):仍挂着的在途态就此清掉。
            if (current) accept(slot);
            slot.settle({ ok: true });
            return;
          }
          const fail = failure(result.code, result.message, result.details);
          if (current) {
            if (planning) usePlanStore.getState().setPlanning(false);
            reject(sessionId, fail);
          }
          // 已受理之后连接才断的,turn 的成败由 agent.failed / ultraplan.stage 说,不在这里改判。
          slot.settle(fail);
        });
      });
    },

    submitAnswers: (answers, target) => get().act('answer', { ...target, answers }),

    approveDemo: (target) => get().act('approve_demo', { ...target }),

    reviseDemo: (feedback, target) => get().act('revise_demo', { ...target, userInput: feedback }),

    revisePlan: (feedback, target) => get().act('revise_plan', { ...target, userInput: feedback }),

    startProduction: (acknowledge, target) => {
      const planPath = get().state?.planPath;
      return get().act('start_production', {
        ...target,
        ...(acknowledge ? { acknowledgeApprovals: true } : {}),
        ...(planPath ? { planPath } : {}),
      });
    },

    resumeProduction: (acknowledge) =>
      get().act('resume_production', acknowledge ? { acknowledgeApprovals: true } : {}),

    fixProduction: (acknowledge, target) =>
      get().act('fix_production', {
        ...target,
        ...(acknowledge ? { acknowledgeApprovals: true } : {}),
      }),

    restart: async () => {
      const result = await rest<{ ok?: boolean }>('restart');
      if (!result.ok) return result;
      if (!result.stale) {
        // 200 即「状态已清」:直接落空,再补拉一次对账(ultraplan.cleared 实时事件也会触发)。
        set({ ...EMPTY, sessionId: result.sessionId });
        void get().refresh(result.sessionId);
      }
      return { ok: true };
    },

    submitAcceptance: async (results, round) => {
      if (!activeSessionId()) return failure(CLIENT_ERROR.noSession, '请先选择会话');
      const state = get().state;
      if (!state) return failure(CLIENT_ERROR.noFlow, '当前会话没有进行中的 UltraPlan 流程');
      const result = await rest<{ ultraplan?: unknown; next?: string; failed?: unknown }>(
        'acceptance',
        { id: state.id, rev: state.planRev, round: round ?? state.acceptanceRound, results },
      );
      if (!result.ok) return result;
      // 跨会话响应不能触发随后 fixProduction,否则会修错当前会话。
      if (result.stale) return failure(CLIENT_ERROR.sessionChanged, '已切换会话,请回原会话查看验收结果');
      if (!result.stale) {
        const next = normalizeUltraPlanState(result.body?.ultraplan);
        if (next) set({ state: next, sessionId: result.sessionId });
        void get().refresh(result.sessionId);
      }
      const failed = Array.isArray(result.body?.failed)
        ? result.body.failed.filter((item): item is string => typeof item === 'string')
        : [];
      return { ok: true, next: result.body?.next === 'fix' ? 'fix' : 'done', failed };
    },

    rollbackDemo: async () => {
      if (!activeSessionId()) return failure(CLIENT_ERROR.noSession, '请先选择会话');
      const state = get().state;
      if (!state) return failure(CLIENT_ERROR.noFlow, '当前会话没有进行中的 UltraPlan 流程');
      const result = await rest<{ ultraplan?: unknown }>('rollback_demo', { id: state.id, rev: state.demoIteration });
      if (!result.ok) return result;
      if (!result.stale) {
        const next = normalizeUltraPlanState(result.body?.ultraplan);
        if (next) set({ state: next, sessionId: result.sessionId });
        void get().refresh(result.sessionId);
      }
      return { ok: true };
    },

    noteUserMessage: (ref) => {
      const slot = actSlot;
      if (!slot) return;
      if (ref.id !== slot.id || ref.action !== slot.action) return;
      // rev 两边都有才比:resume_production 不带 rev,后端回显可能是 null / 缺省。
      if (typeof ref.rev === 'number' && slot.rev !== null && ref.rev !== slot.rev) return;
      actSlot = null;
      accept(slot);
      slot.settle({ ok: true });
    },

    applyLiveEvent: (event) => {
      if (!event.type.startsWith('ultraplan.')) return;
      const sessionId = event.sessionId ?? activeSessionId();
      if (!sessionId || activeSessionId() !== sessionId) return;
      const payload = event.payload ?? {};
      if (event.type === 'ultraplan.stage') {
        set({
          live:
            payload.phase === 'running'
              ? {
                  effort: typeof payload.effort === 'string' ? payload.effort : null,
                  thinkingForced: payload.thinkingForced === true,
                }
              : null,
        });
      } else if (event.type === 'ultraplan.started' || event.type === 'ultraplan.cleared') {
        set({ notices: [], live: null });
      } else if (event.type === 'ultraplan.notice' && typeof payload.code === 'string') {
        const notice: UltraPlanNotice = {
          id: typeof payload.id === 'string' ? payload.id : '',
          runId: typeof payload.runId === 'string' ? payload.runId : null,
          code: payload.code,
          message: typeof payload.message === 'string' ? payload.message : '',
        };
        set((st) => ({
          notices: [
            ...st.notices.filter((n) => !(n.code === notice.code && n.runId === notice.runId)),
            notice,
          ].slice(-MAX_NOTICES),
        }));
      }
      void get().refresh(sessionId);
    },

    reset: () => {
      epoch += 1;
      refreshSlot = null;
      // 在途动作就地结掉:ask:execute 的响应要到整轮结束才回,不能让调用方(跨会话不重挂的
      // 状态条等)陪着等别的会话的一整轮。POST 回来时槽已作废,不再碰 store。
      const orphan = actSlot;
      actSlot = null;
      set({ ...EMPTY });
      orphan?.settle(failure(CLIENT_ERROR.sessionChanged, '会话已切换,操作结果请回原会话查看'));
    },
  };
});

// Plan 页签「正在调研…」条跟阶段机走:act() 发起 Planning 时已先点亮,这里补上发起者不是本页
// (刷新 / 重连 / 别的窗口)的那一轮,并在它跑完(waiting / failed / 流程被清)时熄灭。
// 只在「Planning 是否在跑」翻转时动 planStore,不碰 plan 模式自己的置位 / 复位。
useUltraPlanStore.subscribe((next, prev) => {
  const now = isPlanningTurn(next.state);
  if (now !== isPlanningTurn(prev.state)) usePlanStore.getState().setPlanning(now);
});
