import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Check, ChevronsUpDown, Minus, Plus } from 'lucide-react';
import { cn } from '@/lib/cn';

/**
 * F7 wave.5 设置控件族(参考 ui/settings.rs 控件尺寸逐值):
 * SetCard(rounded-10 border bg-sunk)/SetRow(px-16 py-14 1fr auto,title 13 medium+desc 11.5/17 text-2,
 * 末行无边) /SetToggle(32×18 胶囊,sage 开,14 白 knob)/SetSelect(pl-10 pr-6 py-5 胶囊+up-down 箭头,
 * 菜单 min-w-170 行 h-26)/SetStepper(26×26 ± 夹 mono 值格 min-w-30)/SetSlider(4px 轨 12px knob
 * accent 填充)/SetInput(rounded-6 border bg-panel px-8 py-5 12px)/SmBtn(h-26 px-10 rounded-6 border 12px)。
 */

export function SetH1({ children }: { children: ReactNode }) {
  return (
    <h1 className="mb-4 font-serif text-[22px] font-bold text-fg" data-testid="settings-h1">
      {children}
    </h1>
  );
}

export function SetSectionLabel({ children }: { children: ReactNode }) {
  return <div className="mb-2 mt-5 pl-0.5 text-[11px] font-medium text-fg-3">{children}</div>;
}

export function SetCard({ children, testId }: { children: ReactNode; testId?: string }) {
  return (
    <div
      data-testid={testId}
      className="flex flex-col rounded-[10px] border border-edge bg-shell-sunk"
    >
      {children}
    </div>
  );
}

export function SetRow({
  title,
  desc,
  control,
  last = false,
  dim = false,
  testId,
}: {
  title: ReactNode;
  desc?: ReactNode;
  control?: ReactNode;
  last?: boolean;
  dim?: boolean;
  testId?: string;
}) {
  return (
    <div
      data-testid={testId}
      className={cn(
        'flex items-center gap-4 px-4 py-3.5',
        !last && 'border-b border-edge',
        dim && 'opacity-60',
      )}
    >
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex items-center gap-1.5 text-[13px] font-medium text-fg">{title}</div>
        {desc ? <div className="mt-[3px] text-[11.5px] leading-[17px] text-fg-2">{desc}</div> : null}
      </div>
      {control ? <div className="shrink-0">{control}</div> : null}
    </div>
  );
}

/** 32×18 胶囊 toggle(sage 开,14px 白 knob)。 */
export function SetToggle({
  on,
  onChange,
  testId,
  ariaLabel,
}: {
  on: boolean;
  onChange: (next: boolean) => void;
  testId?: string;
  ariaLabel?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={ariaLabel ?? (typeof testId === 'string' ? testId : 'toggle')}
      data-testid={testId}
      onClick={() => onChange(!on)}
      className={cn(
        'flex h-[18px] w-8 shrink-0 items-center rounded-full px-0.5 transition-colors',
        on ? 'justify-end bg-sage' : 'justify-start bg-shell-active',
      )}
    >
      <span className="h-3.5 w-3.5 rounded-full bg-white shadow-[0_1px_3px_rgba(0,0,0,0.18)]" />
    </button>
  );
}

/** 下拉 select(胶囊 + chevrons-up-down;菜单 min-w-170 行 26,选中 accent+accent_bg+check)。 */
export function SetSelect({
  value,
  options,
  onPick,
  testId,
  minWidth = 170,
}: {
  value: string;
  options: Array<{ value: string; label: string }>;
  onPick: (v: string) => void;
  testId?: string;
  minWidth?: number;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [open]);
  const label = options.find((o) => o.value === value)?.label ?? value;
  return (
    <div ref={rootRef} className="relative" data-testid={testId}>
      <button
        type="button"
        data-testid={testId ? `${testId}-trigger` : undefined}
        onClick={() => setOpen((v) => !v)}
        className={cn(
          'flex items-center gap-1 rounded-md border bg-shell-panel py-[5px] pl-2.5 pr-1.5 text-[12px] text-fg',
          open ? 'border-edge-strong' : 'border-edge hover:border-edge-strong',
        )}
      >
        {label}
        <ChevronsUpDown size={11} className="text-fg-3" />
      </button>
      {open && (
        <div
          role="menu"
          className="absolute right-0 z-50 mt-1 flex flex-col rounded-lg border border-edge-strong bg-shell-float p-1 shadow-float"
          style={{ minWidth }}
        >
          {options.map((o) => {
            const active = o.value === value;
            return (
              <button
                key={o.value}
                type="button"
                role="menuitem"
                data-testid={testId ? `${testId}-item-${o.value}` : undefined}
                onClick={() => {
                  setOpen(false);
                  onPick(o.value);
                }}
                className={cn(
                  'flex h-[26px] items-center rounded-[5px] px-2 text-left text-[12px] hover:bg-shell-hover',
                  active ? 'bg-acc-bg text-acc' : 'text-fg-2',
                )}
              >
                <span className="min-w-0 flex-1 truncate">{o.label}</span>
                {active && <Check size={11} className="shrink-0" />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** 26×26 ± 夹 mono 值格(clamp min–max)。 */
export function SetStepper({
  value,
  min,
  max,
  onSet,
  testId,
}: {
  value: number;
  min: number;
  max: number;
  onSet: (v: number) => void;
  testId?: string;
}) {
  const clamped = Math.min(max, Math.max(min, Math.round(value)));
  const btn =
    'flex h-[26px] w-[26px] items-center justify-center text-fg-2 transition-colors hover:bg-shell-hover';
  return (
    <div
      data-testid={testId}
      className="flex items-center overflow-hidden rounded-md border border-edge bg-shell-panel"
    >
      <button
        type="button"
        aria-label="减少"
        data-testid={testId ? `${testId}-minus` : undefined}
        onClick={() => onSet(Math.max(min, clamped - 1))}
        className={btn}
      >
        <Minus size={11} />
      </button>
      <span
        data-testid={testId ? `${testId}-value` : undefined}
        className="flex min-w-[30px] justify-center border-x border-edge py-1 font-code text-[12px] text-fg"
      >
        {clamped}
      </span>
      <button
        type="button"
        aria-label="增加"
        data-testid={testId ? `${testId}-plus` : undefined}
        onClick={() => onSet(Math.min(max, clamped + 1))}
        className={btn}
      >
        <Plus size={11} />
      </button>
    </div>
  );
}

/** 4px 轨 + 12px knob(accent 填充;读数格 28px mono)。 */
export function SetSlider({
  value,
  min,
  max,
  onChange,
  testId,
  showValue = true,
}: {
  value: number;
  min: number;
  max: number;
  onChange: (v: number) => void;
  testId?: string;
  showValue?: boolean;
}) {
  const clamped = Math.min(max, Math.max(min, value));
  return (
    <div className="flex w-[200px] items-center gap-2" data-testid={testId}>
      <input
        type="range"
        aria-label={testId ?? 'slider'}
        data-testid={testId ? `${testId}-range` : undefined}
        className="set-slider flex-1"
        min={min}
        max={max}
        value={clamped}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      {showValue && (
        <span
          data-testid={testId ? `${testId}-value` : undefined}
          className="w-7 shrink-0 text-right font-code text-[11px] text-fg-3"
        >
          {clamped}
        </span>
      )}
    </div>
  );
}

/** 文本输入框(rounded-6 border bg-panel px-2 py-[5px] 12px)。 */
export function SetInput({
  value,
  onChange,
  placeholder,
  width = 280,
  type = 'text',
  testId,
  onCommit,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  width?: number;
  type?: 'text' | 'password';
  testId?: string;
  /** blur/Enter 提交(hex 校验场景;非法值由调用方拒绝) */
  onCommit?: (v: string) => void;
}) {
  return (
    <input
      type={type}
      value={value}
      placeholder={placeholder}
      data-testid={testId}
      onChange={(e) => onChange(e.target.value)}
      onBlur={(e) => onCommit?.(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') onCommit?.((e.target as HTMLInputElement).value);
      }}
      className="rounded-md border border-edge bg-shell-panel px-2 py-[5px] font-code text-[12px] text-fg outline-none placeholder:text-fg-4 focus:border-edge-strong"
      style={{ width }}
    />
  );
}

/** 小按钮(h-26 px-2.5 rounded-6 border 12px;accent 变体实心)。 */
export function SmBtn({
  label,
  onClick,
  accent = false,
  disabled = false,
  testId,
}: {
  label: ReactNode;
  onClick: () => void;
  accent?: boolean;
  disabled?: boolean;
  testId?: string;
}) {
  return (
    <button
      type="button"
      data-testid={testId}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        'flex h-[26px] items-center justify-center gap-1 rounded-md border px-2.5 text-[12px] transition-colors',
        accent
          ? 'border-acc bg-acc text-fg-inv hover:bg-acc-soft'
          : 'border-edge bg-shell-panel text-fg-2 hover:bg-shell-hover',
        disabled && 'cursor-not-allowed opacity-50',
      )}
    >
      {label}
    </button>
  );
}
