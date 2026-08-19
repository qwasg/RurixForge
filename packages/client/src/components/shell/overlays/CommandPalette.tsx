import { useEffect, useMemo, useRef, useState } from 'react';
import { Search } from 'lucide-react';
import { cn } from '@/lib/cn';
import { filterCommands, SECTION_LABELS, type CommandSection } from '@/lib/commands';
import { useOverlayStore } from '@/lib/overlayStore';
import { Kbd } from '../primitives';

/**
 * F7 wave.3 命令面板(参考 ui/overlays.rs render_palette):
 * Ctrl+K 唤起;scrim rgba(42,39,36,0.18);600px 卡挂顶 15% 视口高圆角 12;
 * 分组小节 10px 大写;行 13px py8 选中 accent_bg;↑↓ 循环 / Enter 执行 / Esc 关。
 */
export default function CommandPalette() {
  const open = useOverlayStore((st) => st.palette);
  const close = useOverlayStore((st) => st.close);
  const [query, setQuery] = useState('');
  const [index, setIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const commands = useMemo(() => filterCommands(query), [query]);
  const selected = Math.min(index, Math.max(0, commands.length - 1));

  useEffect(() => {
    if (open) {
      setQuery('');
      setIndex(0);
      // 等一帧再 focus,确保已挂载
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  if (!open) return null;

  const run = (id: string) => {
    close('palette');
    const cmd = commands.find((c) => c.id === id);
    cmd?.run();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setIndex((selected + 1) % Math.max(1, commands.length));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setIndex((selected - 1 + commands.length) % Math.max(1, commands.length));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const cmd = commands[selected];
      if (cmd) run(cmd.id);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      close('palette');
    }
  };

  // 分组小节:按 section 首见处插入
  let lastSection: CommandSection | '' = '';

  return (
    <div
      data-testid="command-palette-scrim"
      className="absolute inset-0 z-50 flex justify-center"
      style={{ background: 'var(--scrim-palette)' }}
      onMouseDown={() => close('palette')}
    >
      <div
        role="dialog"
        aria-label="命令面板"
        className="mt-[15vh] flex h-auto w-[600px] max-w-[92vw] flex-col self-start overflow-hidden rounded-xl border border-edge-strong bg-shell-float shadow-float"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-edge px-3.5 py-3">
          <Search size={14} className="shrink-0 text-fg-3" />
          <input
            ref={inputRef}
            value={query}
            data-testid="command-palette-input"
            onChange={(e) => {
              setQuery(e.target.value);
              setIndex(0);
            }}
            onKeyDown={onKeyDown}
            placeholder="输入命令…"
            className="min-w-0 flex-1 bg-transparent text-[14px] text-fg outline-none placeholder:text-fg-4"
          />
          <Kbd label="Esc" />
        </div>
        <div className="flex max-h-[380px] flex-col overflow-hidden">
          {commands.length === 0 && (
            <p className="p-4 text-[12px] text-fg-4">没有匹配的命令</p>
          )}
          {commands.map((c, i) => {
            const head =
              c.section !== lastSection ? (
                <div
                  key={`sec-${c.section}`}
                  className="px-3.5 pb-0.5 pt-2 text-[10px] font-semibold uppercase text-fg-4"
                >
                  {SECTION_LABELS[c.section]}
                </div>
              ) : null;
            lastSection = c.section;
            return (
              <div key={c.id}>
                {head}
                <button
                  type="button"
                  data-testid={`command-row-${c.id}`}
                  onMouseEnter={() => setIndex(i)}
                  onClick={() => run(c.id)}
                  className={cn(
                    'flex w-full items-center gap-2.5 px-3.5 py-2 text-left text-[13px] text-fg-2',
                    i === selected && 'bg-acc-bg text-fg',
                  )}
                  style={i === selected ? { background: 'var(--accent-bg)' } : undefined}
                >
                  <span className="min-w-0 flex-1 truncate">{c.label}</span>
                  {c.shortcut ? (
                    <Kbd label={c.shortcut} />
                  ) : (
                    i === selected && <Kbd label="↵" />
                  )}
                </button>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
