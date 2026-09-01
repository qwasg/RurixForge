import { describe, expect, it } from 'vitest';
import {
  MD_MAX_BLOCKS_FINAL,
  MD_MAX_BLOCKS_STREAMING,
  parseMarkdownBlocks,
  stripInline,
  visibleBlocks,
} from '@/lib/markdown';

describe('markdown 逐行解析器(参考 parse_markdown_blocks 行为)', () => {
  it('标题/引用/列表/段落/围栏代码', () => {
    const md = '# Title\n\n## Sub\n\n> quote\n\n- item\n* star\n\npara\n\n```rs\nfn main() {}\n```\n';
    const p = parseMarkdownBlocks(md);
    const kinds = p.blocks.map((b) => b.kind);
    expect(kinds).toEqual([
      'heading', 'heading', 'quote', 'listItem', 'listItem', 'paragraph', 'code',
    ]);
    // 原始行文本保留(标记不剥除,参考同)
    expect(p.blocks[0]).toMatchObject({ text: '# Title' });
    expect(p.blocks[2]).toMatchObject({ text: '> quote' });
    expect(p.blocks[3]).toMatchObject({ text: '- item' });
    expect(p.blocks[6]).toMatchObject({ lang: 'rs', lines: ['fn main() {}'] });
  });

  it('表格简化:| 行成 tableRow,分隔行跳过', () => {
    const md = '| a | b |\n|---|---|\n| 1 | 2 |\n';
    const p = parseMarkdownBlocks(md);
    expect(p.blocks).toHaveLength(2);
    expect(p.blocks[0]).toMatchObject({ kind: 'tableRow', cells: ['a', 'b'] });
    expect(p.blocks[1]).toMatchObject({ kind: 'tableRow', cells: ['1', '2'] });
  });

  it('行内剥除:仅 ** 与反引号(表格单元同样剥)', () => {
    expect(stripInline('**粗** `码` 普通')).toBe('粗 码 普通');
    const p = parseMarkdownBlocks('| **x** | `y` |');
    expect(p.blocks[0]).toMatchObject({ kind: 'tableRow', cells: ['x', 'y'] });
  });

  it('未闭合围栏:尾部行并入代码块', () => {
    const p = parseMarkdownBlocks('```ts\nlet a = 1\nlet b = 2\n');
    expect(p.blocks).toHaveLength(1);
    expect(p.blocks[0]).toMatchObject({ kind: 'code', lang: 'ts', lines: ['let a = 1', 'let b = 2'] });
  });

  it('代码块内的 # / - 不被误解析', () => {
    const p = parseMarkdownBlocks('```\n# not heading\n- not list\n```\n- real\n');
    expect(p.blocks[0]).toMatchObject({ kind: 'code', lines: ['# not heading', '- not list'] });
    expect(p.blocks[1]).toMatchObject({ kind: 'listItem' });
  });

  it('空行不产生块', () => {
    const p = parseMarkdownBlocks('a\n\n\n\nb\n');
    expect(p.blocks.map((b) => b.kind)).toEqual(['paragraph', 'paragraph']);
  });

  it('块数上限:流式 200 / 非流式 800,超出附 truncated', () => {
    const md = Array.from({ length: 250 }, (_, i) => `p${i}`).join('\n');
    const p = parseMarkdownBlocks(md);
    const streaming = visibleBlocks(p, true);
    expect(streaming).toHaveLength(MD_MAX_BLOCKS_STREAMING + 1);
    expect(streaming[streaming.length - 1].kind).toBe('truncated');
    const final = visibleBlocks(p, false);
    expect(final).toHaveLength(250); // 250 < 800 不裁
    const big = parseMarkdownBlocks(Array.from({ length: 900 }, (_, i) => `p${i}`).join('\n'));
    const capped = visibleBlocks(big, false);
    expect(capped).toHaveLength(MD_MAX_BLOCKS_FINAL + 1);
  });
});
