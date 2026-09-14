/**
 * D-035:计划文件(`.forge/plans/<名>.plan.md`)解析。
 *
 * 事实源是工作区文件本身,不是聊天记录——用户可在 Plan 页签直接改完再 Build,刷新不丢。
 * 文件形态 = YAML front matter(name/overview/todos)+ Markdown 正文,写方是 agentd
 * `plan_doc.rs`(serde_yaml)。这里只需读回它写出的那个子集,故手写行级解析而不引 YAML 库:
 * 平铺标量 + 一层 todos 列表,两侧共同遵守「值一律压成单行」的约定(见 plan_doc 模块注释)。
 *
 * 解析失败不吞:返回 error 字段,由 PlanTab 如实提示并回落到「按原始文本预览」——
 * 计划正文还在,不该因为 front matter 坏了就整页打不开。
 */

/** 计划文件所在目录(工作区相对,与 agentd plan_doc::PLAN_DIR 同值)。 */
export const PLAN_DIR = '.forge/plans';
/** 计划文件后缀。 */
export const PLAN_EXT = '.plan.md';

export interface PlanTodo {
  id: string;
  content: string;
  /** pending 为缺省;真实执行状态看会话待办(按 planTodoId 映射)。 */
  status: string;
}

export interface PlanFile {
  name: string;
  overview: string;
  todos: PlanTodo[];
  /** front matter 之后的 Markdown 正文。 */
  body: string;
  /** 解析失败原因(此时 body = 原始全文,name 回落文件名)。 */
  error?: string;
}

/** 是否为计划文件路径(与 agentd plan_doc::is_plan_path 同判据)。 */
export function isPlanPath(path: string): boolean {
  const norm = path.replace(/\\/g, '/');
  if (!norm.startsWith(`${PLAN_DIR}/`)) return false;
  const tail = norm.slice(PLAN_DIR.length + 1);
  return (
    tail.length > PLAN_EXT.length &&
    tail.endsWith(PLAN_EXT) &&
    !tail.includes('/') &&
    !tail.includes('..')
  );
}

/** 路径 → 回落展示名(front matter 读出来之前用)。 */
export function planNameFromPath(path: string): string {
  const file = path.replace(/\\/g, '/').split('/').pop() ?? path;
  return file.replace(/\.plan\.md$/i, '') || path;
}

/**
 * YAML 标量反引号:serde_yaml 只在必要时加引号,故三种形态都要认。
 * 双引号里是 JSON 转义;单引号里 `''` 表示一个 `'`。
 */
function unquote(raw: string): string {
  const v = raw.trim();
  if (v.length >= 2 && v.startsWith('"') && v.endsWith('"')) {
    try {
      return JSON.parse(v) as string;
    } catch {
      return v.slice(1, -1);
    }
  }
  if (v.length >= 2 && v.startsWith("'") && v.endsWith("'")) {
    return v.slice(1, -1).replace(/''/g, "'");
  }
  return v;
}

/** `key: value` 行 → [key, value];非该形态返回 null。 */
function splitKey(line: string): [string, string] | null {
  const m = /^([A-Za-z_][A-Za-z0-9_-]*):(.*)$/.exec(line.trim());
  return m ? [m[1], m[2]] : null;
}

interface FrontMatter {
  name: string;
  overview: string;
  todos: PlanTodo[];
}

type TodoDraft = Partial<Record<'id' | 'content' | 'status', string>>;

function isTodoKey(k: string): k is 'id' | 'content' | 'status' {
  return k === 'id' || k === 'content' || k === 'status';
}

function parseFrontMatter(lines: string[]): FrontMatter {
  const fm: FrontMatter = { name: '', overview: '', todos: [] };
  const drafts: TodoDraft[] = [];
  let inTodos = false;

  for (const line of lines) {
    if (line.trim() === '') continue;
    const listItem = /^\s*-\s*(.*)$/.exec(line);
    // 顶层键(无缩进、非列表项)结束 todos 段。
    if (!listItem && !/^\s/.test(line)) {
      inTodos = false;
      const kv = splitKey(line);
      if (!kv) continue;
      const [key, rest] = kv;
      if (key === 'todos') inTodos = true;
      else if (key === 'name') fm.name = unquote(rest);
      else if (key === 'overview') fm.overview = unquote(rest);
      continue;
    }
    if (!inTodos) continue;
    if (listItem) {
      // 新条目起头:`- id: x`(serde_yaml 不缩进列表项,手写文件常缩进,两种都认)。
      const draft: TodoDraft = {};
      const kv = splitKey(listItem[1]);
      if (kv && isTodoKey(kv[0])) draft[kv[0]] = unquote(kv[1]);
      drafts.push(draft);
      continue;
    }
    // 条目的后续字段行(缩进的 `key: value`)。
    const kv = splitKey(line);
    const draft = drafts[drafts.length - 1];
    if (draft && kv && isTodoKey(kv[0])) draft[kv[0]] = unquote(kv[1]);
  }

  for (const d of drafts) {
    if (!d.id || !d.content) continue;
    fm.todos.push({ id: d.id, content: d.content, status: d.status || 'pending' });
  }
  return fm;
}

/** 计划文件全文 → 结构化。front matter 缺失/无 name 时如实回 error,正文照常可读。 */
export function parsePlanFile(text: string, path = ''): PlanFile {
  const raw = text.replace(/^\uFEFF/, '').replace(/\r\n/g, '\n');
  const fallback = (error: string): PlanFile => ({
    name: planNameFromPath(path),
    overview: '',
    todos: [],
    body: raw.trim(),
    error,
  });
  const lines = raw.split('\n');
  if (lines[0]?.trim() !== '---') return fallback('缺 front matter 起始 ---');
  const end = lines.findIndex((l, i) => i > 0 && l.trim() === '---');
  if (end < 0) return fallback('缺 front matter 结束 ---');

  const fm = parseFrontMatter(lines.slice(1, end));
  const body = lines.slice(end + 1).join('\n').trim();
  if (fm.name === '') return { ...fallback('front matter 缺 name'), body };
  return { name: fm.name, overview: fm.overview, todos: fm.todos, body };
}

/** 计划待办完成度:优先取会话待办的实时状态,未物化(Build 前)回落文件里的 status。 */
export function planTodoProgress(
  todos: PlanTodo[],
  statusOf: (planTodoId: string) => string | undefined,
): { done: number; total: number } {
  let done = 0;
  for (const t of todos) {
    const s = statusOf(t.id) ?? t.status;
    if (s === 'completed' || s === 'done') done += 1;
  }
  return { done, total: todos.length };
}
