import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { ArrowUp, Check, ChevronDown, Mic, Plus } from 'lucide-react';
import { AGENT_MODEL } from '../lib/mock';
import { cn } from '../lib/cn';
import { COMPOSER_MODES, type ComposerMode } from '../lib/editorStore';

export interface ComposerProps {
  autoFocus?: boolean;
  placeholder?: string;
  onSend?: (text: string, mode: ComposerMode) => void;
}

/* 模型下拉的假选项(首项为当前模型,见 mock.AGENT_MODEL) */
const MODEL_OPTIONS = [AGENT_MODEL, 'Cursor Grok 4.5 Fast', 'Claude Sonnet 4.5'];

/**
 * 共享输入卡:HomeView / AgentView 共用。
 * rounded-2xl 白卡 + shadow-composer,聚焦时边框略深。
 */
export default function Composer({
  autoFocus = false,
  placeholder = 'Plan, Build. / for skills, @ for context',
  onSend,
}: ComposerProps) {
  const [value, setValue] = useState('');
  const [model, setModel] = useState(AGENT_MODEL);
  const [mode, setMode] = useState<ComposerMode>('build');
  const [menuOpen, setMenuOpen] = useState(false);
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const modeMenuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (autoFocus) taRef.current?.focus();
  }, [autoFocus]);

  /* 随内容自动增高,上限 200px */
  useEffect(() => {
    const ta = taRef.current;
    if (!ta) return;
    ta.style.height = 'auto';
    ta.style.height = `${Math.min(ta.scrollHeight, 200)}px`;
  }, [value]);

  /* 点击外部 / Esc 关闭模型菜单 */
  useEffect(() => {
    if (!menuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
      }
    };
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') setMenuOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [menuOpen]);

  /* 点击外部 / Esc 关闭模式菜单 */
  useEffect(() => {
    if (!modeMenuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (modeMenuRef.current && !modeMenuRef.current.contains(e.target as Node)) {
        setModeMenuOpen(false);
      }
    };
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') setModeMenuOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [modeMenuOpen]);

  const canSend = value.trim().length > 0;

  const send = () => {
    if (!canSend) return;
    onSend?.(value.trim(), mode);
    setValue('');
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Shift+Enter 换行;isComposing 避免中文输入法选词时误发送
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      send();
    }
  };

  return (
    <div className="rounded-2xl bg-white shadow-composer transition-shadow focus-within:shadow-[0_1px_2px_rgba(0,0,0,0.04),0_0_0_1px_#d8d5d0]">
      <textarea
        ref={taRef}
        rows={2}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={onKeyDown}
        placeholder={placeholder}
        className="max-h-[200px] w-full resize-none overflow-y-auto bg-transparent px-4 pt-3.5 text-base text-ink placeholder:text-muted-faint focus:outline-none"
      />
      <div className="flex items-center gap-1.5 px-3 pb-3">
        <button
          type="button"
          title="Add context"
          className="flex h-7 w-7 items-center justify-center rounded-full bg-panel-hover text-ink-soft transition-colors hover:bg-panel-active"
        >
          <Plus className="h-4 w-4" />
        </button>

        {/* composer 五模式切换器(04 §3 / 07 §5) */}
        <div className="relative" ref={modeMenuRef}>
          <button
            type="button"
            data-testid="composer-mode-select"
            onClick={() => setModeMenuOpen((v) => !v)}
            className="flex items-center gap-1 rounded-full bg-panel-hover px-2 py-1 text-xs font-medium text-ink transition-colors hover:bg-panel-active"
          >
            <span>{mode}</span>
            <ChevronDown
              className={cn(
                'h-3.5 w-3.5 text-muted transition-transform',
                modeMenuOpen && 'rotate-180',
              )}
            />
          </button>
          {modeMenuOpen && (
            <div className="absolute bottom-full left-0 mb-2 w-40 rounded-xl bg-white p-1 shadow-pop">
              {COMPOSER_MODES.map((m) => (
                <button
                  key={m}
                  type="button"
                  data-mode={m}
                  onClick={() => {
                    setMode(m);
                    setModeMenuOpen(false);
                  }}
                  className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs text-ink-soft transition-colors hover:bg-panel-hover"
                >
                  <Check
                    className={cn(
                      'h-3.5 w-3.5 shrink-0',
                      m === mode ? 'text-ink' : 'text-transparent',
                    )}
                  />
                  <span>{m}</span>
                </button>
              ))}
            </div>
          )}
        </div>

        <div className="relative" ref={menuRef}>
          <button
            type="button"
            onClick={() => setMenuOpen((v) => !v)}
            className="flex items-center gap-1 rounded-full px-2 py-1 text-xs text-ink-soft transition-colors hover:bg-panel-hover"
          >
            <span className="max-w-[220px] truncate">{model}</span>
            <ChevronDown
              className={cn(
                'h-3.5 w-3.5 text-muted transition-transform',
                menuOpen && 'rotate-180',
              )}
            />
          </button>
          {menuOpen && (
            <div className="absolute bottom-full left-0 mb-2 w-60 rounded-xl bg-white p-1 shadow-pop">
              {MODEL_OPTIONS.map((m) => (
                <button
                  key={m}
                  type="button"
                  onClick={() => {
                    setModel(m);
                    setMenuOpen(false);
                  }}
                  className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs text-ink-soft transition-colors hover:bg-panel-hover"
                >
                  <Check
                    className={cn(
                      'h-3.5 w-3.5 shrink-0',
                      m === model ? 'text-ink' : 'text-transparent',
                    )}
                  />
                  <span className="truncate">{m}</span>
                </button>
              ))}
            </div>
          )}
        </div>

        <div className="ml-auto flex items-center gap-1.5">
          <button
            type="button"
            title="Dictate"
            className="flex h-8 w-8 items-center justify-center rounded-full text-muted transition-colors hover:bg-panel-hover"
          >
            <Mic className="h-4 w-4" />
          </button>
          <button
            type="button"
            title="Send"
            onClick={send}
            disabled={!canSend}
            className="flex h-8 w-8 items-center justify-center rounded-full bg-ink text-white transition-opacity hover:bg-black disabled:opacity-30 disabled:hover:bg-ink"
          >
            <ArrowUp className="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  );
}
