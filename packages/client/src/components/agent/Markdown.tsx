import { cn } from '@/lib/cn';

/**
 * 小型 Markdown 渲染器,覆盖会话/子智能体报告所需的子集:
 * **粗体**、`行内代码`(路径类渲染为蓝色链接,其余为灰底圆角 chip)、
 * [链接](url)、有序/无序列表、表格、# 标题、--- 分隔线。
 */

/* ---------------- 行内解析 ---------------- */

const INLINE_RE = /(\*\*[\s\S]+?\*\*|`[^`]+`|\[[^\]]+\]\([^)\s]+\))/g;

/** 路径/文件类代码(含 / \ 或扩展名)在 Cursor 中渲染为蓝色可点链接。 */
function looksLikePath(code: string): boolean {
  return /[\\/]/.test(code) || /\.[A-Za-z0-9]{1,6}(\b|$)/.test(code);
}

function renderInline(text: string, keyPrefix: string): JSX.Element[] {
  return text
    .split(INLINE_RE)
    .map((part, i) => {
      const key = `${keyPrefix}-${i}`;
      if (!part) return null;
      if (part.startsWith('**') && part.endsWith('**') && part.length > 4) {
        return (
          <strong key={key} className="font-semibold text-ink">
            {renderInline(part.slice(2, -2), key)}
          </strong>
        );
      }
      if (part.startsWith('`') && part.endsWith('`') && part.length > 2) {
        const code = part.slice(1, -1);
        if (looksLikePath(code)) {
          return (
            <a key={key} className="cursor-pointer text-accent-blue hover:underline">
              {code}
            </a>
          );
        }
        return (
          <code key={key} className="rounded-md bg-panel-hover px-1 py-0.5 text-[0.92em] text-ink-soft">
            {code}
          </code>
        );
      }
      const link = part.match(/^\[([^\]]+)\]\(([^)\s]+)\)$/);
      if (link) {
        return (
          <a
            key={key}
            href={link[2]}
            target="_blank"
            rel="noreferrer"
            className="text-accent-blue hover:underline"
          >
            {link[1]}
          </a>
        );
      }
      return <span key={key}>{part}</span>;
    })
    .filter((el): el is JSX.Element => el !== null);
}

/* ---------------- 块级解析 ---------------- */

type Block =
  | { type: 'p'; lines: string[] }
  | { type: 'ul'; items: string[] }
  | { type: 'ol'; items: string[] }
  | { type: 'table'; rows: string[][] }
  | { type: 'heading'; level: number; text: string }
  | { type: 'hr' };

const HEADING_RE = /^#{1,4}\s/;
const OL_RE = /^\d+[.、]\s/;
const UL_RE = /^[-*•]\s/;
const HR_RE = /^-{3,}$/;

function isStructural(line: string): boolean {
  return HEADING_RE.test(line) || OL_RE.test(line) || UL_RE.test(line) || HR_RE.test(line) || line.startsWith('|');
}

function parseBlocks(md: string): Block[] {
  const lines = md.split('\n');
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const trimmed = lines[i].trim();
    if (!trimmed) {
      i++;
      continue;
    }
    if (HEADING_RE.test(trimmed)) {
      const level = trimmed.match(/^#+/)![0].length;
      blocks.push({ type: 'heading', level, text: trimmed.slice(level).trim() });
      i++;
      continue;
    }
    if (HR_RE.test(trimmed)) {
      blocks.push({ type: 'hr' });
      i++;
      continue;
    }
    if (trimmed.startsWith('|')) {
      const rows: string[][] = [];
      while (i < lines.length && lines[i].trim().startsWith('|')) {
        const cells = lines[i]
          .trim()
          .replace(/^\||\|$/g, '')
          .split('|')
          .map((c) => c.trim());
        // 跳过 |---|---| 分隔行
        if (!cells.every((c) => /^:?-{2,}:?$/.test(c))) rows.push(cells);
        i++;
      }
      blocks.push({ type: 'table', rows });
      continue;
    }
    if (OL_RE.test(trimmed)) {
      const items: string[] = [];
      while (i < lines.length && OL_RE.test(lines[i].trim())) {
        items.push(lines[i].trim().replace(OL_RE, ''));
        i++;
      }
      blocks.push({ type: 'ol', items });
      continue;
    }
    if (UL_RE.test(trimmed)) {
      const items: string[] = [];
      while (i < lines.length && UL_RE.test(lines[i].trim())) {
        items.push(lines[i].trim().replace(UL_RE, ''));
        i++;
      }
      blocks.push({ type: 'ul', items });
      continue;
    }
    const plines: string[] = [];
    while (i < lines.length) {
      const t = lines[i].trim();
      if (!t || isStructural(t)) break;
      plines.push(t);
      i++;
    }
    blocks.push({ type: 'p', lines: plines });
  }
  return blocks;
}

/* ---------------- 渲染 ---------------- */

export default function Markdown({ md, className }: { md: string; className?: string }) {
  const blocks = parseBlocks(md);
  return (
    <div className={cn('space-y-3 text-base leading-[1.65] text-ink', className)}>
      {blocks.map((b, bi) => {
        switch (b.type) {
          case 'heading':
            return (
              <div key={bi} className={cn('font-semibold text-ink', b.level <= 2 ? 'text-lg' : 'text-base')}>
                {renderInline(b.text, `h${bi}`)}
              </div>
            );
          case 'hr':
            return <hr key={bi} className="border-line-soft" />;
          case 'p':
            return (
              <p key={bi}>
                {b.lines.map((l, li) => (
                  <span key={li}>
                    {li > 0 && <br />}
                    {renderInline(l, `p${bi}-${li}`)}
                  </span>
                ))}
              </p>
            );
          case 'ul':
            return (
              <ul key={bi} className="space-y-1.5">
                {b.items.map((it, ii) => (
                  <li key={ii} className="flex gap-2.5 pl-1">
                    <span className="shrink-0 text-muted">•</span>
                    <span className="min-w-0">{renderInline(it, `ul${bi}-${ii}`)}</span>
                  </li>
                ))}
              </ul>
            );
          case 'ol':
            return (
              <ol key={bi} className="space-y-1.5">
                {b.items.map((it, ii) => (
                  <li key={ii} className="flex gap-2.5 pl-1">
                    <span className="shrink-0 font-medium text-ink">{ii + 1}.</span>
                    <span className="min-w-0">{renderInline(it, `ol${bi}-${ii}`)}</span>
                  </li>
                ))}
              </ol>
            );
          case 'table': {
            const [head, ...body] = b.rows;
            if (!head) return null;
            return (
              <div key={bi} className="overflow-x-auto rounded-lg border border-line-soft">
                <table className="w-full border-collapse text-sm">
                  <thead>
                    <tr>
                      {head.map((c, ci) => (
                        <th
                          key={ci}
                          className="border-b border-r border-line-soft bg-panel px-2.5 py-1.5 text-left font-semibold text-ink last:border-r-0"
                        >
                          {renderInline(c, `th${bi}-${ci}`)}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {body.map((r, ri) => (
                      <tr key={ri} className="last:[&>td]:border-b-0">
                        {r.map((c, ci) => (
                          <td
                            key={ci}
                            className="border-b border-r border-line-soft px-2.5 py-1.5 align-top text-ink-soft last:border-r-0"
                          >
                            {renderInline(c, `td${bi}-${ri}-${ci}`)}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          }
        }
      })}
    </div>
  );
}
