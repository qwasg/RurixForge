import { useMemo } from 'react';
import { parseMarkdownBlocks, stripInline, visibleBlocks, type MdBlock } from '@/lib/markdown';

/**
 * F7 wave.4 MarkdownFlat(参考 render_markdown_flat + render_one_block 样式逐条对齐):
 * 根 = flex col gap-1(4px)13px;标题 pt-1 bold 14px;引用 pl-3 2px 左轨 text_2;
 * 列表 • 前缀;表格等宽 12px flex 行;围栏代码 bg_sunk 圆角 6 mono;
 * 行内剥除 ** 与反引号。流式 text_2 / 非流式 text。
 *
 * 差异留痕:参考非流式代码块接自研 syntax 高亮;本仓不接高亮库(纯样式渲染,
 * 需求驱动再评,不伪造)。
 */
function Block({ block, streaming }: { block: MdBlock; streaming: boolean }) {
  switch (block.kind) {
    case 'heading':
      return (
        <div className="pt-1 text-[14px] font-bold">{stripInline(block.text.trimStart())}</div>
      );
    case 'quote':
      return (
        <div className="border-l-2 border-edge pl-3 text-fg-2">
          {stripInline(block.text.trimStart())}
        </div>
      );
    case 'listItem':
      return (
        <div className="flex gap-1.5">
          <span className="shrink-0 text-fg-3">•</span>
          <span className="min-w-0 flex-1">{stripInline(block.text.trimStart())}</span>
        </div>
      );
    case 'tableRow':
      return (
        <div className="flex gap-2 text-[12px]">
          {block.cells.map((c, i) => (
            <span key={i} className="min-w-0 flex-1">
              {c}
            </span>
          ))}
        </div>
      );
    case 'code':
      return (
        <div
          data-testid="md-code"
          className="rounded-md bg-shell-sunk px-2.5 py-2 font-code text-shell-code text-fg-2"
        >
          {block.lines.map((l, i) => (
            <div key={i}>{l === '' ? ' ' : l}</div>
          ))}
        </div>
      );
    case 'truncated':
      return <div className="pt-1 text-[11px] text-fg-4">（内容较长，已截断显示）</div>;
    case 'paragraph':
      return <div>{stripInline(block.text.trimStart())}</div>;
  }
}

export default function MarkdownFlat({
  text,
  streaming = false,
}: {
  text: string;
  streaming?: boolean;
}) {
  const blocks = useMemo(
    () => visibleBlocks(parseMarkdownBlocks(text), streaming),
    [text, streaming],
  );
  return (
    <div
      data-testid="markdown-flat"
      className={`flex flex-col gap-1 text-[13px] ${streaming ? 'text-fg-2' : 'text-fg'}`}
    >
      {blocks.map((b, i) => (
        <Block key={i} block={b} streaming={streaming} />
      ))}
    </div>
  );
}
