import type { Language } from '@codemirror/language';
import { langExtensionForPath } from './cmLang';

type Parser = Language['parser'];

/**
 * 对话代码块静态高亮:复用文件编辑器的语言包(cmLang 按需动态加载),不起编辑器实例,
 * 只用 lezer 解析树 + tagHighlighter 产出按行的 token 段;颜色走 index.css 的 .tok-* 类
 * (--code-* 变量,随主题切换)。无匹配语言 / 超长代码返回 null,调用方按纯文本渲染。
 */

export interface HlSpan {
  text: string;
  cls: string | null;
}
export type HlLine = HlSpan[];

/** 超过此长度不解析(整块按纯文本渲染,免得一段巨型输出卡住主线程)。 */
const MAX_HIGHLIGHT_CHARS = 20_000;
const CACHE_LIMIT = 200;

/** 围栏语言名 → cmLang 识别的伪文件名。 */
const FENCE_ALIAS: Record<string, string> = {
  ts: 'x.ts',
  typescript: 'x.ts',
  tsx: 'x.tsx',
  js: 'x.js',
  javascript: 'x.js',
  jsx: 'x.jsx',
  mjs: 'x.mjs',
  cjs: 'x.cjs',
  json: 'x.json',
  jsonc: 'x.json',
  json5: 'x.json',
  rust: 'x.rs',
  rs: 'x.rs',
  rx: 'x.rx',
  md: 'x.md',
  markdown: 'x.md',
  powershell: 'x.ps1',
  ps1: 'x.ps1',
  pwsh: 'x.ps1',
  toml: 'x.toml',
  yaml: 'x.yaml',
  yml: 'x.yml',
  sh: 'x.sh',
  bash: 'x.sh',
  shell: 'x.sh',
  zsh: 'x.sh',
  console: 'x.sh',
  css: 'x.css',
  html: 'x.html',
  xml: 'x.html',
};

export function supportsHighlight(lang: string): boolean {
  return lang.toLowerCase() in FENCE_ALIAS;
}

const parserCache = new Map<string, Promise<Parser | null>>();

function parserFor(lang: string): Promise<Parser | null> {
  const key = lang.toLowerCase();
  const path = FENCE_ALIAS[key];
  if (!path) return Promise.resolve(null);
  let hit = parserCache.get(key);
  if (!hit) {
    hit = (async () => {
      const [{ Language, LanguageSupport }, ext] = await Promise.all([
        import('@codemirror/language'),
        langExtensionForPath(path),
      ]);
      if (ext instanceof Language) return ext.parser;
      if (ext instanceof LanguageSupport) return ext.language.parser;
      return null;
    })().catch(() => null);
    parserCache.set(key, hit);
  }
  return hit;
}

const resultCache = new Map<string, HlLine[]>();

let highlighterPromise: Promise<{
  highlightTree: typeof import('@lezer/highlight').highlightTree;
  highlighter: import('@lezer/highlight').Highlighter;
}> | null = null;

function loadHighlighter() {
  highlighterPromise ??= import('@lezer/highlight').then(({ highlightTree, tagHighlighter, tags: t }) => ({
    highlightTree,
    highlighter: tagHighlighter([
      {
        tag: [t.keyword, t.controlKeyword, t.operatorKeyword, t.definitionKeyword, t.moduleKeyword, t.modifier],
        class: 'tok-keyword',
      },
      { tag: [t.string, t.special(t.string), t.regexp, t.character, t.escape], class: 'tok-string' },
      { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], class: 'tok-comment' },
      { tag: [t.number, t.integer, t.float, t.bool, t.null, t.atom], class: 'tok-number' },
      {
        tag: [t.function(t.variableName), t.function(t.propertyName), t.function(t.definition(t.variableName))],
        class: 'tok-func',
      },
      { tag: [t.typeName, t.className, t.namespace, t.standard(t.typeName)], class: 'tok-type' },
      { tag: [t.propertyName, t.attributeName, t.labelName], class: 'tok-prop' },
      { tag: [t.meta, t.annotation, t.processingInstruction, t.heading], class: 'tok-meta' },
      { tag: [t.punctuation, t.operator, t.bracket, t.separator], class: 'tok-punct' },
    ]),
  }));
  return highlighterPromise;
}

function pushSpan(lines: HlLine[], text: string, cls: string | null) {
  const parts = text.split('\n');
  parts.forEach((part, i) => {
    if (i > 0) lines.push([]);
    if (part !== '') lines[lines.length - 1].push({ text: part, cls });
  });
}

/** 高亮一段代码 → 按行 token 段;不支持的语言/超长/解析失败 → null。 */
export async function highlightCode(code: string, lang: string): Promise<HlLine[] | null> {
  if (code.length > MAX_HIGHLIGHT_CHARS || !supportsHighlight(lang)) return null;
  const key = `${lang.toLowerCase()}\u0000${code}`;
  const cached = resultCache.get(key);
  if (cached) return cached;
  const [parser, { highlightTree, highlighter }] = await Promise.all([parserFor(lang), loadHighlighter()]);
  if (!parser) return null;
  try {
    const tree = parser.parse(code);
    const lines: HlLine[] = [[]];
    let pos = 0;
    highlightTree(tree, highlighter, (from, to, classes) => {
      if (from > pos) pushSpan(lines, code.slice(pos, from), null);
      pushSpan(lines, code.slice(from, to), classes);
      pos = to;
    });
    if (pos < code.length) pushSpan(lines, code.slice(pos), null);
    if (resultCache.size >= CACHE_LIMIT) {
      const oldest = resultCache.keys().next().value;
      if (oldest !== undefined) resultCache.delete(oldest);
    }
    resultCache.set(key, lines);
    return lines;
  } catch {
    return null;
  }
}
