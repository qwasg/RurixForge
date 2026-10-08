/**
 * 对话正文 Markdown 解析(逐行块解析 + 行内解析;不引 Markdown 库)。
 *
 * 块级:``` / ~~~ 围栏代码(lang = 围栏后首词)、# 标题(1–6 级,渲染封顶 4 级)、> 引用(连续行合并)、
 * -/*\/+ 与 1. 1) 列表(按缩进栈求嵌套层级,[ ] / [x] 任务项)、| 表格(连续行合为一张表,
 * 第二行为分隔行时首行作表头并取对齐)、---/*** 分隔线,其余每行一段(保留模型输出的换行语义)。
 * 标记符号在解析阶段剥掉,渲染层不再看到 `# ` / `> ` / `- `。
 *
 * 行内:`code`、[文字](链接)、<https://…> 与裸 https:// 链接、**粗** / __粗__、*斜* / _斜_、~~删~~。
 * 块数上限由渲染层裁:流式 200 / 非流式 800;源行软上限 50000。
 */

export const MARKDOWN_PARSE_MAX_LINES = 50_000;
/** 流式 200 / 非流式 800 块。 */
export const MD_MAX_BLOCKS_STREAMING = 200;
export const MD_MAX_BLOCKS_FINAL = 800;

export type MdAlign = 'left' | 'center' | 'right' | null;

export type MdBlock =
  | { kind: 'heading'; level: 1 | 2 | 3 | 4; text: string }
  | { kind: 'quote'; text: string }
  | {
      kind: 'listItem';
      ordered: boolean;
      /** 有序列表的序号原文(如 `3.`);无序为 ''。 */
      marker: string;
      /** 嵌套层级(0 起,封顶 4)。 */
      depth: number;
      task: 'done' | 'todo' | null;
      text: string;
    }
  | { kind: 'table'; header: string[] | null; align: MdAlign[]; rows: string[][] }
  | { kind: 'hr' }
  | { kind: 'paragraph'; text: string }
  | { kind: 'code'; lang: string; lines: string[] }
  | { kind: 'truncated' };

export interface ParsedMarkdown {
  blocks: MdBlock[];
  lineCount: number;
}

/** 剥掉行内标记只留文字(表格对齐计算、纯文本场景用)。 */
export function stripInline(s: string): string {
  return s
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\*\*|__|~~/g, '')
    .replace(/`/g, '');
}

const FENCE_RE = /^(```+|~~~+)\s*([^\s`]*)/;
const HEADING_RE = /^(#{1,6})\s+(.*?)\s*#*\s*$/;
const HR_RE = /^(?:(?:\*\s*){3,}|(?:-\s*){3,}|(?:_\s*){3,})$/;
const LIST_RE = /^([ \t]*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const TASK_RE = /^\[([ xX])\]\s+(.*)$/;

function indentWidth(ws: string): number {
  let n = 0;
  for (const ch of ws) n += ch === '\t' ? 4 : 1;
  return n;
}

/** 表格行切格:去首尾竖线,按未转义 | 切,\| 还原为字面竖线。 */
function splitCells(line: string): string[] {
  const body = line.trim().replace(/^\|/, '').replace(/\|$/, '');
  const cells: string[] = [];
  let cur = '';
  for (let i = 0; i < body.length; i++) {
    const ch = body[i];
    if (ch === '\\' && body[i + 1] === '|') {
      cur += '|';
      i++;
    } else if (ch === '|') {
      cells.push(cur.trim());
      cur = '';
    } else {
      cur += ch;
    }
  }
  cells.push(cur.trim());
  return cells;
}

function isSeparatorRow(cells: string[]): boolean {
  return cells.length > 0 && cells.every((c) => /^:?-{1,}:?$/.test(c.replace(/\s/g, '')));
}

function alignOf(cell: string): MdAlign {
  const c = cell.replace(/\s/g, '');
  const left = c.startsWith(':');
  const right = c.endsWith(':');
  if (left && right) return 'center';
  if (right) return 'right';
  if (left) return 'left';
  return null;
}

export function parseMarkdownBlocks(input: string): ParsedMarkdown {
  const split = input.replace(/\r\n?/g, '\n').split('\n');
  if (split.length > 0 && split[split.length - 1] === '') split.pop();
  const totalLines = split.length;
  const lines = split.slice(0, MARKDOWN_PARSE_MAX_LINES);
  const blocks: MdBlock[] = [];
  let fence: { marker: string; lang: string; lines: string[] } | null = null;
  /** 当前连续列表的缩进栈(非列表行打断即清空)。 */
  let listIndents: number[] = [];

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trim();

    if (fence) {
      // 闭合围栏:整行只有同种围栏符,且不短于开围栏
      const ch = fence.marker[0];
      const closes =
        trimmed.length >= fence.marker.length && [...trimmed].every((c) => c === ch);
      if (closes) {
        blocks.push({ kind: 'code', lang: fence.lang, lines: fence.lines });
        fence = null;
      } else {
        fence.lines.push(line);
      }
      continue;
    }

    const fenceOpen = FENCE_RE.exec(trimmed);
    if (fenceOpen) {
      fence = { marker: fenceOpen[1], lang: fenceOpen[2] ?? '', lines: [] };
      listIndents = [];
      continue;
    }
    if (trimmed === '') continue;

    const list = LIST_RE.exec(line);
    if (list && !HR_RE.test(trimmed)) {
      const indent = indentWidth(list[1]);
      while (listIndents.length > 0 && indent < listIndents[listIndents.length - 1]) listIndents.pop();
      if (listIndents.length === 0 || indent > listIndents[listIndents.length - 1]) listIndents.push(indent);
      const depth = Math.min(4, listIndents.length - 1);
      const ordered = /\d/.test(list[2]);
      const task = TASK_RE.exec(list[3]);
      blocks.push({
        kind: 'listItem',
        ordered,
        marker: ordered ? list[2] : '',
        depth,
        task: task ? (task[1] === ' ' ? 'todo' : 'done') : null,
        text: task ? task[2] : list[3],
      });
      continue;
    }
    listIndents = [];

    const heading = HEADING_RE.exec(trimmed);
    if (heading) {
      const level = Math.min(4, heading[1].length) as 1 | 2 | 3 | 4;
      blocks.push({ kind: 'heading', level, text: heading[2] });
      continue;
    }
    if (HR_RE.test(trimmed)) {
      blocks.push({ kind: 'hr' });
      continue;
    }
    if (trimmed.startsWith('>')) {
      const quoteLines = [trimmed.replace(/^>\s?/, '')];
      while (i + 1 < lines.length && lines[i + 1].trim().startsWith('>')) {
        i++;
        quoteLines.push(lines[i].trim().replace(/^>\s?/, ''));
      }
      blocks.push({ kind: 'quote', text: quoteLines.join('\n') });
      continue;
    }
    if (trimmed.startsWith('|')) {
      const rows = [splitCells(trimmed)];
      while (i + 1 < lines.length && lines[i + 1].trim().startsWith('|')) {
        i++;
        rows.push(splitCells(lines[i]));
      }
      if (rows.length >= 2 && isSeparatorRow(rows[1])) {
        blocks.push({ kind: 'table', header: rows[0], align: rows[1].map(alignOf), rows: rows.slice(2) });
      } else {
        const body = rows.filter((r) => !isSeparatorRow(r));
        blocks.push({ kind: 'table', header: null, align: [], rows: body });
      }
      continue;
    }
    blocks.push({ kind: 'paragraph', text: trimmed });
  }
  if (fence) blocks.push({ kind: 'code', lang: fence.lang, lines: fence.lines });
  if (totalLines > MARKDOWN_PARSE_MAX_LINES) blocks.push({ kind: 'truncated' });
  return { blocks, lineCount: totalLines };
}

/** 渲染窗口:按 streaming 取块上限,超出 → 附带 truncated 尾块。 */
export function visibleBlocks(parsed: ParsedMarkdown, streaming: boolean): MdBlock[] {
  const max = streaming ? MD_MAX_BLOCKS_STREAMING : MD_MAX_BLOCKS_FINAL;
  if (parsed.blocks.length <= max) return parsed.blocks;
  return [...parsed.blocks.slice(0, max), { kind: 'truncated' }];
}

// ---------- 行内 ----------

export type InlineNode =
  | { t: 'text'; v: string }
  | { t: 'code'; v: string }
  | { t: 'strong'; c: InlineNode[] }
  | { t: 'em'; c: InlineNode[] }
  | { t: 'del'; c: InlineNode[] }
  | { t: 'link'; href: string; c: InlineNode[] };

const WORD_RE = /[\p{L}\p{N}_]/u;
/** 裸链接结尾要剥掉的标点(含中文全角标点)。 */
const URL_TRAIL_RE = /[.,;:!?'"。，；：！？、）)\]》」』]+$/;

function isWordChar(ch: string | undefined): boolean {
  return ch !== undefined && WORD_RE.test(ch);
}

/** 从 open 处(指向 `[`)找配对的 `]`,支持一层嵌套括号。 */
function matchBracket(src: string, open: number, l: string, r: string): number {
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    const ch = src[i];
    if (ch === '\\') {
      i++;
      continue;
    }
    if (ch === l) depth++;
    else if (ch === r) {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** 找成对强调符的闭合位置:闭合符前不是空白;`_` 系还要求闭合符后不是单词字符。 */
function findClose(src: string, from: number, delim: string): number {
  let i = src.indexOf(delim, from);
  while (i !== -1) {
    const before = src[i - 1];
    const after = src[i + delim.length];
    const okBefore = before !== undefined && !/\s/.test(before);
    const okAfter = delim[0] !== '_' || !isWordChar(after);
    // 单个 * / _ 不能是双写符的一半
    const single = delim.length === 1 && (src[i + 1] === delim || before === delim);
    if (okBefore && okAfter && !single && i > from) return i;
    i = src.indexOf(delim, i + 1);
  }
  return -1;
}

export function parseInline(src: string): InlineNode[] {
  const out: InlineNode[] = [];
  let text = '';
  const flush = () => {
    if (text !== '') out.push({ t: 'text', v: text });
    text = '';
  };
  let i = 0;
  while (i < src.length) {
    const ch = src[i];
    const rest = src.slice(i);

    if (ch === '\\' && i + 1 < src.length && /[\\`*_{}[\]()#+\-.!|~<>]/.test(src[i + 1])) {
      text += src[i + 1];
      i += 2;
      continue;
    }

    if (ch === '`') {
      const run = /^`+/.exec(rest)![0];
      const close = src.indexOf(run, i + run.length);
      if (close !== -1) {
        flush();
        const inner = src.slice(i + run.length, close);
        out.push({ t: 'code', v: inner.length > 1 && inner.startsWith(' ') && inner.endsWith(' ') ? inner.slice(1, -1) : inner });
        i = close + run.length;
        continue;
      }
      text += run;
      i += run.length;
      continue;
    }

    if (ch === '[') {
      const closeBracket = matchBracket(src, i, '[', ']');
      if (closeBracket !== -1 && src[closeBracket + 1] === '(') {
        const closeParen = matchBracket(src, closeBracket + 1, '(', ')');
        if (closeParen !== -1) {
          const label = src.slice(i + 1, closeBracket);
          const target = src.slice(closeBracket + 2, closeParen).trim();
          const href = target.replace(/\s+"[^"]*"$/, '').replace(/^<(.*)>$/, '$1');
          if (href !== '') {
            flush();
            out.push({ t: 'link', href, c: parseInline(label === '' ? href : label) });
            i = closeParen + 1;
            continue;
          }
        }
      }
    }

    if (ch === '<') {
      const auto = /^<(https?:\/\/[^\s>]+)>/i.exec(rest);
      if (auto) {
        flush();
        out.push({ t: 'link', href: auto[1], c: [{ t: 'text', v: auto[1] }] });
        i += auto[0].length;
        continue;
      }
    }

    if ((ch === 'h' || ch === 'H') && !isWordChar(src[i - 1])) {
      const bare = /^https?:\/\/[^\s<>"'`，。；、）】》」』]+/i.exec(rest);
      if (bare) {
        const url = bare[0].replace(URL_TRAIL_RE, '');
        flush();
        out.push({ t: 'link', href: url, c: [{ t: 'text', v: url }] });
        i += url.length;
        continue;
      }
    }

    const two = src.slice(i, i + 2);
    if (two === '**' || two === '__' || two === '~~') {
      const leftOk = two !== '__' || !isWordChar(src[i - 1]);
      const openOk = src[i + 2] !== undefined && !/\s/.test(src[i + 2]);
      const close = leftOk && openOk ? findClose(src, i + 2, two) : -1;
      if (close !== -1) {
        flush();
        const kids = parseInline(src.slice(i + 2, close));
        out.push(two === '~~' ? { t: 'del', c: kids } : { t: 'strong', c: kids });
        i = close + 2;
        continue;
      }
      text += two;
      i += 2;
      continue;
    }

    if (ch === '*' || ch === '_') {
      const leftOk = ch !== '_' || !isWordChar(src[i - 1]);
      const openOk = src[i + 1] !== undefined && !/\s/.test(src[i + 1]);
      const close = leftOk && openOk ? findClose(src, i + 1, ch) : -1;
      if (close !== -1) {
        flush();
        out.push({ t: 'em', c: parseInline(src.slice(i + 1, close)) });
        i = close + 1;
        continue;
      }
    }

    text += ch;
    i++;
  }
  flush();
  return out;
}

// ---------- 链接目标 ----------

export type LinkTarget =
  | { kind: 'external'; href: string }
  | { kind: 'workspace'; path: string }
  | { kind: 'other'; href: string };

/**
 * 链接目标分类:http(s)/mailto 交给浏览器(桌面端由主进程转系统浏览器);
 * 其余视作文件路径——file:// 与 #L12 / :12:3 行号后缀剥掉,落在工作区根下的绝对路径换算成相对路径,
 * 相对路径去掉 ./ 前缀;工作区外的绝对路径原样交给后端(confined 校验如实报错)。
 */
export function classifyLink(href: string, workspaceRoot: string | null): LinkTarget {
  const raw = href.trim();
  if (/^(https?:|mailto:)/i.test(raw)) return { kind: 'external', href: raw };
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(raw) && !/^file:/i.test(raw)) return { kind: 'other', href: raw };
  let p = raw.replace(/^file:\/\/\/?/i, '');
  try {
    p = decodeURI(p);
  } catch {
    // 非法转义序列:按原文处理
  }
  p = p.replace(/#.*$/, '').replace(/:(\d+)(:\d+)?$/, '').replace(/\\/g, '/');
  if (p === '') return { kind: 'other', href: raw };
  if (workspaceRoot) {
    const root = workspaceRoot.replace(/^\\\\\?\\/, '').replace(/\\/g, '/').replace(/\/+$/, '');
    if (p.toLowerCase().startsWith(`${root.toLowerCase()}/`)) {
      return { kind: 'workspace', path: p.slice(root.length + 1) };
    }
  }
  return { kind: 'workspace', path: p.replace(/^\.\//, '') };
}
