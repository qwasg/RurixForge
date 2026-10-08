import {
  cloneElement,
  forwardRef,
  isValidElement,
  useId,
  useState,
  type InputHTMLAttributes,
  type ReactElement,
  type ReactNode,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
} from 'react';
import { cn } from '@/lib/utils';

const control =
  'w-full min-w-0 rounded-md border border-input bg-card text-sm text-foreground placeholder:text-muted-foreground/70 focus-visible:border-ring focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30 disabled:cursor-not-allowed disabled:opacity-60';

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(function Input(
  { className, ...props },
  ref,
) {
  return <input ref={ref} className={cn(control, 'h-8 px-2.5 read-only:bg-muted/40', className)} {...props} />;
});

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement>>(function Textarea(
  { className, ...props },
  ref,
) {
  return <textarea ref={ref} className={cn(control, 'min-h-20 px-2.5 py-1.5 leading-5', className)} {...props} />;
});

export const Select = forwardRef<HTMLSelectElement, SelectHTMLAttributes<HTMLSelectElement>>(function Select(
  { className, ...props },
  ref,
) {
  return <select ref={ref} className={cn(control, 'h-8 px-2', className)} {...props} />;
});

export const Checkbox = forwardRef<
  HTMLInputElement,
  Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> & { label?: ReactNode; hint?: ReactNode }
>(function Checkbox({ label, hint, className, ...props }, ref) {
  const box = <input ref={ref} type="checkbox" className="size-3.5 shrink-0 accent-info" {...props} />;
  if (!label) return box;
  return (
    <label className={cn('inline-flex cursor-pointer select-none items-start gap-2 text-sm', props.disabled && 'cursor-not-allowed opacity-60', className)}>
      <span className="flex h-5 items-center">{box}</span>
      <span className="flex flex-col">
        <span>{label}</span>
        {hint ? <span className="text-xs text-muted-foreground">{hint}</span> : null}
      </span>
    </label>
  );
});

/** 标签 + 控件 + 提示/错误。单个子元素没有 id 时自动分配，使 <label htmlFor> 生效。 */
export function Field({
  label,
  hint,
  error,
  required,
  className,
  children,
}: {
  label?: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  required?: boolean;
  className?: string;
  children: ReactNode;
}) {
  const autoId = useId();
  let id: string | undefined;
  let content = children;
  if (isValidElement(children)) {
    const el = children as ReactElement<{ id?: string }>;
    id = el.props.id ?? autoId;
    if (!el.props.id) content = cloneElement(el, { id });
  }
  return (
    <div className={cn('flex min-w-0 flex-col gap-1', className)}>
      {label ? (
        <label htmlFor={id} className="text-xs font-medium text-foreground/80">
          {label}
          {required ? <span className="text-danger"> *</span> : null}
        </label>
      ) : null}
      {content}
      {error ? (
        <p className="text-xs text-danger">{error}</p>
      ) : hint ? (
        <p className="text-xs leading-4 text-muted-foreground">{hint}</p>
      ) : null}
    </div>
  );
}

export interface Option<T extends string | number> {
  value: T;
  label: ReactNode;
  /** 过滤用的纯文本（label 不是字符串时提供）。 */
  text?: string;
  hint?: ReactNode;
  disabled?: boolean;
}

/** 多选（复选框列表）。inline：横排小选项；否则带边框的可滚动列表，可选过滤框。 */
export function CheckboxGroup<T extends string | number>({
  options,
  value,
  onChange,
  inline = false,
  filterable = false,
  emptyText = '暂无可选项',
  className,
  'aria-label': ariaLabel,
}: {
  options: Option<T>[];
  value: T[];
  onChange: (next: T[]) => void;
  inline?: boolean;
  filterable?: boolean;
  emptyText?: string;
  className?: string;
  'aria-label'?: string;
}) {
  const [filter, setFilter] = useState('');
  const selected = new Set<T>(value);
  const toggle = (v: T) => onChange(selected.has(v) ? value.filter((x) => x !== v) : [...value, v]);
  const q = filter.trim().toLowerCase();
  const shown = q
    ? options.filter((o) =>
        [String(o.value), o.text ?? (typeof o.label === 'string' ? o.label : '')].some((s) => s.toLowerCase().includes(q)),
      )
    : options;

  const items = shown.map((o) => (
    <label
      key={String(o.value)}
      className={cn(
        'flex cursor-pointer select-none items-center gap-2 rounded px-1.5 py-1 text-sm hover:bg-accent',
        o.disabled && 'cursor-not-allowed opacity-50',
        inline && 'border border-input py-0.5',
        inline && selected.has(o.value) && 'border-info/60 bg-info/5',
      )}
    >
      <input
        type="checkbox"
        className="size-3.5 shrink-0 accent-info"
        checked={selected.has(o.value)}
        disabled={o.disabled}
        onChange={() => toggle(o.value)}
      />
      <span className="min-w-0 truncate">{o.label}</span>
      {o.hint ? <span className="ml-auto shrink-0 pl-2 text-xs text-muted-foreground">{o.hint}</span> : null}
    </label>
  ));

  if (inline) {
    return (
      <div role="group" aria-label={ariaLabel} className={cn('flex flex-wrap gap-1.5', className)}>
        {options.length === 0 ? <span className="text-xs text-muted-foreground">{emptyText}</span> : items}
      </div>
    );
  }
  return (
    <div role="group" aria-label={ariaLabel} className={cn('rounded-md border border-input bg-card', className)}>
      {filterable && options.length > 8 ? (
        <div className="border-b p-1.5">
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="筛选…"
            aria-label="筛选选项"
            className="h-7"
          />
        </div>
      ) : null}
      <div className="max-h-44 overflow-y-auto p-1">
        {shown.length === 0 ? <p className="px-1.5 py-2 text-xs text-muted-foreground">{emptyText}</p> : items}
      </div>
      {value.length > 0 ? (
        <div className="flex items-center justify-between border-t px-2 py-1 text-xs text-muted-foreground">
          <span>已选 {value.length} 项</span>
          <button type="button" className="hover:text-foreground" onClick={() => onChange([])}>
            清空
          </button>
        </div>
      ) : null}
    </div>
  );
}
