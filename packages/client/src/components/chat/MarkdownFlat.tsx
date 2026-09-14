import { useMemo } from 'react';
import { parseMarkdownBlocks, stripInline, visibleBlocks, type MdBlock } from '@/lib/markdown';
import EntityRefText from './EntityRefText';

/**
 * F7 wave.4 MarkdownFlat(参考 render_markdown_flat + render_one_block 样式逐条对齐):
 * 根 = flex col gap-1(4px)13px;标题 pt-1 bold 14px;引用 pl-3 2px 左轨 text_2;
 * 列表 • 前缀;表格等宽 12px flex 行;围栏代码 bg_sunk 圆角 6 mono;
 * 行内剥除 ** 与反引号。流式 text_2 / 非流式 text。
 *
 * 差异留痕:参考非流式代码块接自研 syntax 高亮;本仓不接高亮库(纯样式渲染,
 * 需求驱动再评,不伪造)。
 *
 * 2026-09-03 用户指令(正文加黑加粗)留痕:新增 strong 档 —— 助手正文(中文)一律
 * text 全黑 + font-bold,且流式期间不退灰,与英文过程链的两档灰拉开三级层级;
 * 围栏代码块在 strong 档下显式回正常字重(bold 等宽代码糊成一团,可读性倒退)。
 * strong 只作用于「助手对用户说的话」,子代理 PROMPT/SUMMARY 等辅助文本不传该档。
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
          className="rounded-md bg-shell-sunk px-2.5 py-2 font-code text-shell-code font-normal text-fg-2"
        >
          {block.lines.map((l, i) => (
            <div key={i}>{l === '' ? ' ' : l}</div>
          ))}
        </div>
      );
    case 'truncated':
      return <div className="pt-1 text-[11px] text-fg-4">（内容较长，已截断显示）</div>;
    case 'paragraph':
      return (
        <div>
          <EntityRefText text={stripInline(block.text.trimStart())} />
        </div>
      );
  }
}

export default function MarkdownFlat({
  text,
  streaming = false,
  strong = false,
}: {
  text: string;
  streaming?: boolean;
  /** 助手正文档:全黑 + 加粗,流式期间不退灰(2026-09-03 用户指令)。 */
  strong?: boolean;
}) {
  const blocks = useMemo(
    () => visibleBlocks(parseMarkdownBlocks(text), streaming),
    [text, streaming],
  );
  const tone = strong ? 'font-bold text-fg' : streaming ? 'text-fg-2' : 'text-fg';
  return (
    <div
      data-testid="markdown-flat"
      className={`flex flex-col gap-1 text-[13px] ${tone}`}
    >
      {blocks.map((b, i) => (
        <Block key={i} block={b} streaming={streaming} />
      ))}
    </div>
  );
}
