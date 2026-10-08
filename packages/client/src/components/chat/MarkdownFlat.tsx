import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { Check, CheckSquare, Copy, Square } from 'lucide-react';
import { cn } from '@/lib/cn';
import { copyText } from '@/lib/clipboard';
import { highlightCode, type HlLine } from '@/lib/codeHighlight';
import {
  classifyLink,
  parseInline,
  parseMarkdownBlocks,
  visibleBlocks,
  type InlineNode,
  type MdAlign,
  type MdBlock,
} from '@/lib/markdown';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { displayRoot, useWorkspaceStore } from '@/lib/workspaceStore';
import EntityRefText from './EntityRefText';
import StreamEnter from './StreamEnter';

/**
 * 对话正文 Markdown 渲染(解析见 lib/markdown.ts):标题分级、引用、有序/无序/嵌套/任务列表、
 * 整表、分隔线、围栏代码(语言标签 + 复制 + 非流式静态高亮);行内粗/斜/删/行内代码/链接。
 * http(s) 链接新窗口打开(桌面端由主进程转系统浏览器),工作区路径在工作台文件 tab 打开;
 * 纯文本段保留 #123 实体跳转。
 *
 * 2026-09-03 用户指令(正文加黑加粗)留痕:strong 档 —— 助手正文(中文)一律 text 全黑 + font-bold,
 * 且流式期间不退灰;行内代码与代码块显式回正常字重(bold 等宽代码糊成一团),链接靠强调色区分。
 * strong 只作用于「助手对用户说的话」,子代理 PROMPT/SUMMARY 等辅助文本不传该档。
 */

const BULLETS = ['•', '◦', '▪', '•', '◦'];

function openWorkspaceLink(href: string) {
  const ws = useWorkspaceStore.getState();
  const root = ws.workspaces.find((w) => w.id === ws.activeWorkspaceId)?.root ?? null;
  const target = classifyLink(href, root ? displayRoot(root) : null);
  if (target.kind === 'workspace') useWorkbenchStore.getState().openFile(target.path);
}

function Inline({ nodes }: { nodes: InlineNode[] }) {
  return (
    <>
      {nodes.map((n, i) => {
        switch (n.t) {
          case 'text':
            return <EntityRefText key={i} text={n.v} />;
          case 'code':
            return (
              <code
                key={i}
                className="rounded bg-shell-sunk px-1 py-px font-code text-[0.92em] font-normal text-fg-2"
              >
                {n.v}
              </code>
            );
          case 'strong':
            return (
              <strong key={i} className="font-bold">
                <Inline nodes={n.c} />
              </strong>
            );
          case 'em':
            return (
              <em key={i} className="italic">
                <Inline nodes={n.c} />
              </em>
            );
          case 'del':
            return (
              <del key={i} className="text-fg-3">
                <Inline nodes={n.c} />
              </del>
            );
          case 'link': {
            const target = classifyLink(n.href, null);
            const cls =
              'text-acc underline decoration-acc/40 underline-offset-2 transition-colors hover:decoration-acc';
            if (target.kind === 'external') {
              return (
                <a key={i} data-testid="md-link" href={target.href} target="_blank" rel="noreferrer noopener" className={cls}>
                  <Inline nodes={n.c} />
                </a>
              );
            }
            if (target.kind === 'workspace') {
              return (
                <button
                  key={i}
                  type="button"
                  data-testid="md-link"
                  title={`在工作台打开 ${n.href}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    openWorkspaceLink(n.href);
                  }}
                  className={cn(cls, 'break-all text-left')}
                >
                  <Inline nodes={n.c} />
                </button>
              );
            }
            return (
              <span key={i} title={n.href} className="text-fg-2 underline decoration-dotted underline-offset-2">
                <Inline nodes={n.c} />
              </span>
            );
          }
        }
      })}
    </>
  );
}

function InlineText({ text }: { text: string }) {
  const nodes = useMemo(() => parseInline(text), [text]);
  return <Inline nodes={nodes} />;
}

function CodeBlock({ lang, lines, streaming }: { lang: string; lines: string[]; streaming: boolean }) {
  const code = lines.join('\n');
  const [hl, setHl] = useState<HlLine[] | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (streaming || lang === '') {
      setHl(null);
      return;
    }
    let alive = true;
    void highlightCode(code, lang).then((r) => {
      if (alive) setHl(r);
    });
    return () => {
      alive = false;
    };
  }, [code, lang, streaming]);

  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(t);
  }, [copied]);

  const rows: ReactNode[] =
    hl !== null && hl.length === lines.length
      ? hl.map((spans, i) => (
          <div key={i}>
            {spans.length === 0
              ? ' '
              : spans.map((s, j) => (
                  <span key={j} className={s.cls ?? undefined}>
                    {s.text}
                  </span>
                ))}
          </div>
        ))
      : lines.map((l, i) => <div key={i}>{l === '' ? ' ' : l}</div>);

  return (
    <div data-testid="md-code" className="group/code overflow-hidden rounded-lg border border-edge bg-shell-sunk font-normal">
      <div className="flex h-7 items-center gap-2 border-b border-edge px-2.5 text-[11px] text-fg-3">
        <span className="font-code">{lang === '' ? 'text' : lang}</span>
        <span className="flex-1" />
        <button
          type="button"
          data-testid="md-code-copy"
          aria-label="复制代码"
          title="复制代码"
          onClick={() => void copyText(code, '代码').then((ok) => setCopied(ok))}
          className="flex h-5 items-center gap-1 rounded px-1.5 text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2"
        >
          {copied ? <Check size={11} className="text-sage" /> : <Copy size={11} />}
          {copied ? '已复制' : '复制'}
        </button>
      </div>
      <pre className="overflow-x-auto px-3 py-2 font-code text-shell-code text-fg-2">{rows}</pre>
    </div>
  );
}

const ALIGN_CLASS: Record<Exclude<MdAlign, null>, string> = {
  left: 'text-left',
  center: 'text-center',
  right: 'text-right',
};

function Table({ block }: { block: Extract<MdBlock, { kind: 'table' }> }) {
  const alignOf = (i: number) => {
    const a = block.align[i];
    return a ? ALIGN_CLASS[a] : 'text-left';
  };
  return (
    <div className="overflow-x-auto py-0.5">
      <table data-testid="md-table" className="w-max min-w-full border-collapse text-[12.5px]">
        {block.header && (
          <thead>
            <tr className="bg-shell-sunk">
              {block.header.map((c, i) => (
                <th key={i} className={cn('border border-edge px-2.5 py-1 font-semibold', alignOf(i))}>
                  <InlineText text={c} />
                </th>
              ))}
            </tr>
          </thead>
        )}
        <tbody>
          {block.rows.map((row, r) => (
            <tr key={r}>
              {row.map((c, i) => (
                <td key={i} className={cn('border border-edge px-2.5 py-1 align-top', alignOf(i))}>
                  <InlineText text={c} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const HEADING_CLASS: Record<1 | 2 | 3 | 4, string> = {
  1: 'pt-2 text-[17px] font-bold',
  2: 'pt-1.5 text-[15.5px] font-bold',
  3: 'pt-1 text-[14px] font-bold',
  4: 'pt-1 text-[13px] font-semibold text-fg-2',
};

function Block({ block, streaming }: { block: MdBlock; streaming: boolean }) {
  switch (block.kind) {
    case 'heading':
      return (
        <div className={HEADING_CLASS[block.level]}>
          <InlineText text={block.text} />
        </div>
      );
    case 'quote':
      return (
        <div className="border-l-2 border-edge-strong pl-3 text-fg-2">
          {block.text.split('\n').map((l, i) => (
            <div key={i}>
              <InlineText text={l} />
            </div>
          ))}
        </div>
      );
    case 'listItem': {
      const done = block.task === 'done';
      return (
        <div data-testid="md-list-item" className="flex gap-1.5" style={{ paddingLeft: block.depth * 16 }}>
          {block.task !== null ? (
            <span className="mt-[3px] shrink-0 text-fg-3" aria-label={done ? '已完成' : '未完成'}>
              {done ? <CheckSquare size={12} className="text-sage" /> : <Square size={12} />}
            </span>
          ) : (
            <span
              className={cn(
                'shrink-0 text-fg-3',
                block.ordered && 'min-w-[1.4em] text-right font-normal tabular-nums',
              )}
            >
              {block.ordered ? block.marker : BULLETS[block.depth]}
            </span>
          )}
          <span className={cn('min-w-0 flex-1', done && 'text-fg-3 line-through')}>
            <InlineText text={block.text} />
          </span>
        </div>
      );
    }
    case 'table':
      return <Table block={block} />;
    case 'hr':
      return <hr className="my-1 border-edge" />;
    case 'code':
      return <CodeBlock lang={block.lang} lines={block.lines} streaming={streaming} />;
    case 'truncated':
      return <div className="pt-1 text-[11px] text-fg-4">（内容较长，已截断显示）</div>;
    case 'paragraph':
      return (
        <div>
          <InlineText text={block.text} />
        </div>
      );
  }
}

export default function MarkdownFlat({
  text,
  streaming = false,
  strong = false,
  animate = false,
}: {
  text: string;
  streaming?: boolean;
  /** 助手正文档:全黑 + 加粗,流式期间不退灰(2026-09-03 用户指令)。 */
  strong?: boolean;
  /**
   * D-047:所在消息仍在流式 → 新出现的 markdown 块(段落 / 列表项 / 代码块…)自上而下入场。
   * 每块挂载时定夺(StreamEnter),已在屏上的块不因流式结束或文本增长重播。
   */
  animate?: boolean;
}) {
  const blocks = useMemo(() => visibleBlocks(parseMarkdownBlocks(text), streaming), [text, streaming]);
  const tone = strong ? 'font-bold text-fg' : streaming ? 'text-fg-2' : 'text-fg';
  return (
    <div data-testid="markdown-flat" className={`flex flex-col gap-1 text-[13px] leading-[1.6] ${tone}`}>
      {blocks.map((b, i) => (
        <StreamEnter key={i} active={animate}>
          <Block block={b} streaming={streaming} />
        </StreamEnter>
      ))}
    </div>
  );
}
