import { useState } from 'react';
import { Check, ChevronDown, Circle, Terminal, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { toolStatus, type ChatBlock } from '@/lib/timeline';

type ToolBlock = Extract<ChatBlock, { kind: 'tool' }>;

function commandParts(block: ToolBlock): { command: string; cwd: string } {
  try {
    const value = JSON.parse(block.args) as Record<string, unknown>;
    const raw = value.command ?? value.cmd;
    const command = Array.isArray(raw)
      ? raw.filter((part): part is string => typeof part === 'string').join(' ')
      : typeof raw === 'string'
        ? raw
        : block.args;
    return { command, cwd: typeof value.cwd === 'string' ? value.cwd : '' };
  } catch {
    return { command: block.args, cwd: '' };
  }
}

/** Codex commandExecution：命令、实时输出与退出码在同一个可折叠终端块里。 */
export default function CommandBlock({ block }: { block: ToolBlock }) {
  const status = toolStatus(block);
  const [open, setOpen] = useState(status === 'running');
  const { command, cwd } = commandParts(block);
  const output = block.output ?? block.result ?? block.error ?? '';

  return (
    <div
      data-testid={`command-block-${block.toolCallId}`}
      className="my-1 overflow-hidden rounded-lg border border-edge bg-shell-sunk"
    >
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-2 px-2.5 py-2 text-left"
      >
        <Terminal size={12} className="shrink-0 text-fg-3" />
        <span className="min-w-0 flex-1 truncate font-code text-[11.5px] text-fg-2" title={command}>
          {command || 'shell'}
        </span>
        {status === 'running' ? (
          <Circle size={9} className="shrink-0 animate-pulse fill-current text-acc" />
        ) : status === 'done' ? (
          <Check size={11} className="shrink-0 text-fg-3" />
        ) : (
          <X size={11} className="shrink-0 text-fg-3" />
        )}
        <ChevronDown
          size={11}
          className={cn('shrink-0 text-fg-4 transition-transform', open && 'rotate-180')}
        />
      </button>
      {open && (
        <div className="border-t border-edge">
          {cwd && (
            <div className="border-b border-edge px-2.5 py-1 font-code text-[10px] text-fg-4">
              {cwd}
            </div>
          )}
          <pre
            data-testid="command-output"
            className="max-h-[300px] overflow-auto whitespace-pre-wrap break-words px-2.5 py-2 font-code text-[11px] leading-[17px] text-fg-3"
          >
            {output || (status === 'running' ? 'Running…' : 'No output')}
          </pre>
          {block.exitCode !== undefined && (
            <div
              data-testid="command-exit-code"
              className="border-t border-edge px-2.5 py-1 text-right font-code text-[10px] text-fg-4"
            >
              exit {block.exitCode}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
