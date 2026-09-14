/**
 * F7 wave.4 逐行 markdown 解析器 TS 移植(参考 ui/markdown_preview.rs parse_markdown_blocks
 * 行为逐条对齐;G-F7-4)。
 *
 * - ``` 围栏代码块(lang=围栏后首个词);#/##/### 标题(同粗同号,原始行文本保留);
 *   > 引用;-/* 列表;|a|b| 表格简化(全 -/:/空白 单元行=分隔行,跳过);其余=段落。
 * - 行内剥除 strip_inline:仅去 ** 与反引号(参考同;标题#/引用>/列表- 标记按参考原样保留
 *   在文本中,列表渲染时前加 •——参考 render 路径即此行为,如实移植)。
 * - 块数上限由渲染层裁:流式 200 / 非流式 800(参考 render_markdown_flat);源行软上限 50000。
 */

export const MARKDOWN_PARSE_MAX_LINES = 50_000;
/** 流式 200 / 非流式 800(参考 render_markdown_flat max_blocks)。 */
export const MD_MAX_BLOCKS_STREAMING = 200;
export const MD_MAX_BLOCKS_FINAL = 800;

export type MdBlock =
  | { kind: 'heading'; text: string }
  | { kind: 'quote'; text: string }
  | { kind: 'listItem'; text: string }
  | { kind: 'tableRow'; cells: string[] }
  | { kind: 'paragraph'; text: string }
  | { kind: 'code'; lang: string; lines: string[] }
  | { kind: 'truncated' };

export interface ParsedMarkdown {
  blocks: MdBlock[];
  lineCount: number;
}

/** 行内剥除(参考 strip_inline:仅 ** 与反引号)。 */
export function stripInline(s: string): string {
  return s.replace(/\*\*/g, '').replace(/`/g, '');
}

export function parseMarkdownBlocks(input: string): ParsedMarkdown {
  // 对齐 Rust str::lines():末尾单个 \n 不产空行(原生 split('\n') 会多一 '' 尾元素,剥掉)
  const split = input.split('\n');
  if (split.length > 0 && split[split.length - 1] === '') split.pop();
  const lines = split;
  const totalLines = lines.length;
  const blocks: MdBlock[] = [];
  let codeBuf: string[] | null = null;
  let codeLang = '';

  for (const line of lines.slice(0, MARKDOWN_PARSE_MAX_LINES)) {
    const trimmed = line.trimStart();
    if (trimmed.startsWith('```')) {
      const lang = trimmed.slice(3).split(/\s+/)[0] ?? '';
      if (codeBuf !== null) {
        blocks.push({ kind: 'code', lang: codeLang, lines: codeBuf });
        codeBuf = null;
        codeLang = '';
      } else {
        codeLang = lang;
        codeBuf = [];
      }
      continue;
    }
    if (codeBuf !== null) {
      codeBuf.push(line);
      continue;
    }
    if (trimmed === '') continue;
    if (trimmed.startsWith('### ') || trimmed.startsWith('## ') || trimmed.startsWith('# ')) {
      blocks.push({ kind: 'heading', text: line });
    } else if (trimmed.startsWith('> ')) {
      blocks.push({ kind: 'quote', text: line });
    } else if (trimmed.startsWith('- ') || trimmed.startsWith('* ')) {
      blocks.push({ kind: 'listItem', text: line });
    } else if (trimmed.startsWith('|')) {
      const cells = trimmed
        .replace(/^\|+|\|+$/g, '')
        .split('|')
        .map((c) => stripInline(c.trim()));
      // 分隔行(|---|:---:|)跳过
      if (cells.every((c) => [...c].every((ch) => ch === '-' || ch === ':' || /\s/.test(ch)))) {
        continue;
      }
      blocks.push({ kind: 'tableRow', cells });
    } else {
      blocks.push({ kind: 'paragraph', text: line });
    }
  }
  if (codeBuf !== null) blocks.push({ kind: 'code', lang: codeLang, lines: codeBuf });
  if (totalLines > MARKDOWN_PARSE_MAX_LINES) blocks.push({ kind: 'truncated' });
  return { blocks, lineCount: totalLines };
}

/** 渲染窗口:按 streaming 取块上限,超出 → 附带 truncated 尾块(参考截断提示行)。 */
export function visibleBlocks(parsed: ParsedMarkdown, streaming: boolean): MdBlock[] {
  const max = streaming ? MD_MAX_BLOCKS_STREAMING : MD_MAX_BLOCKS_FINAL;
  if (parsed.blocks.length <= max) return parsed.blocks;
  return [...parsed.blocks.slice(0, max), { kind: 'truncated' }];
}
