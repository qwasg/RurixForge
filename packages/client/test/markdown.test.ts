import { describe, expect, it } from 'vitest';
import {
  classifyLink,
  MD_MAX_BLOCKS_FINAL,
  MD_MAX_BLOCKS_STREAMING,
  parseInline,
  parseMarkdownBlocks,
  stripInline,
  visibleBlocks,
} from '@/lib/markdown';

describe('markdown 块解析', () => {
  it('标记在解析阶段剥掉:标题分级 / 引用合并 / 列表 / 段落 / 围栏代码', () => {
    const md = '# Title\n\n## Sub\n\n> q1\n> q2\n\n- item\n* star\n\npara\n\n```rs\nfn main() {}\n```\n';
    const p = parseMarkdownBlocks(md);
    expect(p.blocks.map((b) => b.kind)).toEqual([
      'heading', 'heading', 'quote', 'listItem', 'listItem', 'paragraph', 'code',
    ]);
    expect(p.blocks[0]).toMatchObject({ level: 1, text: 'Title' });
    expect(p.blocks[1]).toMatchObject({ level: 2, text: 'Sub' });
    expect(p.blocks[2]).toMatchObject({ text: 'q1\nq2' });
    expect(p.blocks[3]).toMatchObject({ ordered: false, depth: 0, text: 'item' });
    expect(p.blocks[6]).toMatchObject({ lang: 'rs', lines: ['fn main() {}'] });
  });

  it('标题 5/6 级封顶为 4 级;行尾闭合 # 去掉', () => {
    const p = parseMarkdownBlocks('###### 深 ##\n');
    expect(p.blocks[0]).toMatchObject({ kind: 'heading', level: 4, text: '深' });
  });

  it('有序 / 嵌套 / 任务列表', () => {
    const md = '1. 一\n2) 二\n   - 子\n     - 孙\n- [ ] 待办\n- [x] 完成\n';
    const b = parseMarkdownBlocks(md).blocks;
    expect(b[0]).toMatchObject({ ordered: true, marker: '1.', depth: 0, text: '一' });
    expect(b[1]).toMatchObject({ ordered: true, marker: '2)', depth: 0 });
    expect(b[2]).toMatchObject({ ordered: false, depth: 1, text: '子' });
    expect(b[3]).toMatchObject({ depth: 2, text: '孙' });
    expect(b[4]).toMatchObject({ depth: 0, task: 'todo', text: '待办' });
    expect(b[5]).toMatchObject({ task: 'done', text: '完成' });
  });

  it('表格:连续行合为一张表,分隔行定表头与对齐;\\| 转义保留字面竖线', () => {
    const md = '| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | x\\|y |\n| 3 | 4 | 5 |\n';
    const p = parseMarkdownBlocks(md);
    expect(p.blocks).toHaveLength(1);
    expect(p.blocks[0]).toMatchObject({
      kind: 'table',
      header: ['a', 'b', 'c'],
      align: ['left', 'center', 'right'],
      rows: [['1', '2', 'x|y'], ['3', '4', '5']],
    });
  });

  it('无分隔行的表格:无表头,全部作数据行', () => {
    const p = parseMarkdownBlocks('| a | b |\n| 1 | 2 |\n');
    expect(p.blocks[0]).toMatchObject({ kind: 'table', header: null, rows: [['a', 'b'], ['1', '2']] });
  });

  it('分隔线不被当成列表', () => {
    const p = parseMarkdownBlocks('上\n---\n***\n下\n');
    expect(p.blocks.map((b) => b.kind)).toEqual(['paragraph', 'hr', 'hr', 'paragraph']);
  });

  it('未闭合围栏:尾部行并入代码块;围栏内的 # / - / ```lang 不被误解析', () => {
    const p = parseMarkdownBlocks('```ts\nlet a = 1\nlet b = 2\n');
    expect(p.blocks).toEqual([{ kind: 'code', lang: 'ts', lines: ['let a = 1', 'let b = 2'] }]);
    const q = parseMarkdownBlocks('````\n# not heading\n```js\n- not list\n````\n- real\n');
    expect(q.blocks[0]).toMatchObject({ kind: 'code', lines: ['# not heading', '```js', '- not list'] });
    expect(q.blocks[1]).toMatchObject({ kind: 'listItem', text: 'real' });
  });

  it('空行不产生块;CRLF 与 LF 同解析', () => {
    expect(parseMarkdownBlocks('a\n\n\n\nb\n').blocks.map((b) => b.kind)).toEqual(['paragraph', 'paragraph']);
    expect(parseMarkdownBlocks('a\r\nb\r\n').blocks).toHaveLength(2);
  });

  it('块数上限:流式 200 / 非流式 800,超出附 truncated', () => {
    const md = Array.from({ length: 250 }, (_, i) => `p${i}`).join('\n');
    const p = parseMarkdownBlocks(md);
    const streaming = visibleBlocks(p, true);
    expect(streaming).toHaveLength(MD_MAX_BLOCKS_STREAMING + 1);
    expect(streaming[streaming.length - 1].kind).toBe('truncated');
    expect(visibleBlocks(p, false)).toHaveLength(250);
    const big = parseMarkdownBlocks(Array.from({ length: 900 }, (_, i) => `p${i}`).join('\n'));
    expect(visibleBlocks(big, false)).toHaveLength(MD_MAX_BLOCKS_FINAL + 1);
  });

  it('stripInline:去掉强调/代码/链接标记只留文字', () => {
    expect(stripInline('**粗** `码` [链](http://a) ~~删~~')).toBe('粗 码 链 删');
  });
});

describe('markdown 行内解析', () => {
  it('粗 / 斜 / 删 / 行内代码', () => {
    expect(parseInline('a **b** *c* ~~d~~ `e`')).toEqual([
      { t: 'text', v: 'a ' },
      { t: 'strong', c: [{ t: 'text', v: 'b' }] },
      { t: 'text', v: ' ' },
      { t: 'em', c: [{ t: 'text', v: 'c' }] },
      { t: 'text', v: ' ' },
      { t: 'del', c: [{ t: 'text', v: 'd' }] },
      { t: 'text', v: ' ' },
      { t: 'code', v: 'e' },
    ]);
  });

  it('snake_case 与 2 * 3 * 4 不误判为斜体;代码里的 ** 原样保留', () => {
    expect(parseInline('snake_case_name')).toEqual([{ t: 'text', v: 'snake_case_name' }]);
    expect(parseInline('2 * 3 * 4')).toEqual([{ t: 'text', v: '2 * 3 * 4' }]);
    expect(parseInline('`a**b**`')).toEqual([{ t: 'code', v: 'a**b**' }]);
  });

  it('链接:[文字](地址)、<自动链接>、裸 https 链接去掉结尾标点', () => {
    expect(parseInline('见 [文档](https://x.dev/a) 。')).toEqual([
      { t: 'text', v: '见 ' },
      { t: 'link', href: 'https://x.dev/a', c: [{ t: 'text', v: '文档' }] },
      { t: 'text', v: ' 。' },
    ]);
    expect(parseInline('<https://a.b/c>')[0]).toEqual({
      t: 'link',
      href: 'https://a.b/c',
      c: [{ t: 'text', v: 'https://a.b/c' }],
    });
    expect(parseInline('打开 https://a.b/c。')).toEqual([
      { t: 'text', v: '打开 ' },
      { t: 'link', href: 'https://a.b/c', c: [{ t: 'text', v: 'https://a.b/c' }] },
      { t: 'text', v: '。' },
    ]);
  });

  it('链接文字里可嵌套强调;带括号的路径完整匹配', () => {
    const nodes = parseInline('[**粗链**](src/a (1).ts)');
    expect(nodes).toEqual([
      { t: 'link', href: 'src/a (1).ts', c: [{ t: 'strong', c: [{ t: 'text', v: '粗链' }] }] },
    ]);
  });

  it('未闭合标记按字面输出', () => {
    expect(parseInline('**半截')).toEqual([{ t: 'text', v: '**半截' }]);
    expect(parseInline('[无链接]')).toEqual([{ t: 'text', v: '[无链接]' }]);
  });
});

describe('classifyLink', () => {
  it('http / mailto 为外链', () => {
    expect(classifyLink('https://a.dev', null)).toEqual({ kind: 'external', href: 'https://a.dev' });
    expect(classifyLink('mailto:a@b.c', null).kind).toBe('external');
  });

  it('工作区根下的绝对路径换算成相对路径(大小写/斜杠不敏感,剥 \\\\?\\ 前缀)', () => {
    expect(
      classifyLink('D:/RurixForge/projects/cs/SourceMedia/x.mp4', '\\\\?\\D:\\RurixForge\\projects\\cs'),
    ).toEqual({ kind: 'workspace', path: 'SourceMedia/x.mp4' });
    expect(classifyLink('file:///d:/rurixforge/projects/cs/a.ts', 'D:\\RurixForge\\projects\\cs')).toEqual({
      kind: 'workspace',
      path: 'a.ts',
    });
  });

  it('相对路径去 ./,剥行号与锚点;工作区外绝对路径原样交后端', () => {
    expect(classifyLink('./src/a.ts:12:3', null)).toEqual({ kind: 'workspace', path: 'src/a.ts' });
    expect(classifyLink('src/a.ts#L10', null)).toEqual({ kind: 'workspace', path: 'src/a.ts' });
    expect(classifyLink('C:/Windows/x.txt', 'D:/ws')).toEqual({ kind: 'workspace', path: 'C:/Windows/x.txt' });
  });

  it('其他协议不当文件打开', () => {
    expect(classifyLink('codex://threads/new', null).kind).toBe('other');
  });
});
