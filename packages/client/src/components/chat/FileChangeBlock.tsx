import { useMemo, useState } from 'react';
import { ChevronDown, FileDiff } from 'lucide-react';
import { cn } from '@/lib/cn';
import type { ChatBlock } from '@/lib/timeline';

type ToolBlock = Extract<ChatBlock, { kind: 'tool' }>;
type FileChange = NonNullable<ToolBlock['changes']>[number];

function fallbackChanges(block: ToolBlock): FileChange[] {
  if (block.changes && block.changes.length > 0) return block.changes;
  try {
    const parsed = JSON.parse(block.args) as { changes?: FileChange[]; path?: string; diff?: string };
    if (Array.isArray(parsed.changes)) return parsed.changes;
    if (typeof parsed.path === 'string') {
      return [{ path: parsed.path, kind: 'update', diff: parsed.diff }];
    }
  } catch {
    // 老事件没有结构化 changes 时保留空态，不伪造路径。
  }
  return [];
}

function Diff({ text }: { text: string }) {
  return (
    <pre className="overflow-auto whitespace-pre font-code text-[10.5px] leading-[16px]">
      {text.split('\n').map((line, index) => (
        <div
          key={`${index}:${line}`}
          className={cn(
            'min-w-max px-2.5',
            line.startsWith('+') && !line.startsWith('+++') && 'bg-sage-bg text-sage',
            line.startsWith('-') && !line.startsWith('---') && 'bg-danger-bg text-danger',
            !line.startsWith('+') && !line.startsWith('-') && 'text-fg-4',
          )}
        >
          {line || ' '}
        </div>
      ))}
    </pre>
  );
}

/**
 * Codex fileChange：逐文件显示变更类型及统一 diff。
 * D-047:live(所在消息仍在流式且变更在写)时摘要文字扫光。
 */
export default function FileChangeBlock({ block, live = false }: { block: ToolBlock; live?: boolean }) {
  const changes = useMemo(() => fallbackChanges(block), [block]);
  const [open, setOpen] = useState(false);
  const summary =
    changes.length === 0
      ? 'File changes'
      : `${changes.length} ${changes.length === 1 ? 'file' : 'files'} changed`;

  return (
    <div
      data-testid={`file-change-block-${block.toolCallId}`}
      className="my-1 overflow-hidden rounded-lg border border-edge bg-shell-sunk"
    >
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-2 px-2.5 py-2 text-left"
      >
        <FileDiff size={12} className="text-fg-3" />
        <span className="min-w-0 flex-1">
          <span className={cn('block w-fit max-w-full text-[11.5px]', live ? 'forge-shimmer' : 'text-fg-2')}>
            {summary}
          </span>
        </span>
        <ChevronDown
          size={11}
          className={cn('text-fg-4 transition-transform', open && 'rotate-180')}
        />
      </button>
      {open && (
        <div className="flex flex-col border-t border-edge">
          {changes.length === 0 && (
            <div className="px-2.5 py-2 text-[11px] text-fg-4">No structured diff</div>
          )}
          {changes.map((change, index) => (
            <div key={`${change.path}:${index}`} className={index > 0 ? 'border-t border-edge' : ''}>
              <div className="flex items-center gap-2 px-2.5 py-1.5">
                <span className="min-w-0 flex-1 truncate font-code text-[11px] text-fg-2">
                  {change.path}
                </span>
                {change.kind && (
                  <span className="rounded bg-shell-active px-1.5 py-0.5 text-[9.5px] uppercase text-fg-4">
                    {change.kind}
                  </span>
                )}
              </div>
              {change.diff && <Diff text={change.diff} />}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
