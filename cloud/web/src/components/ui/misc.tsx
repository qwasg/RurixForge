import { Check, Copy, Info, LoaderCircle, TriangleAlert } from 'lucide-react';
import { useState, type HTMLAttributes, type ReactNode } from 'react';
import { errorMessage } from '@/lib/api/client';
import { copyText } from '@/lib/browser';
import { formatPercent } from '@/lib/format';
import { useToast } from '@/lib/toast';
import { cn } from '@/lib/utils';
import { Button } from './button';

export function PageHeader({ title, description, actions }: { title: ReactNode; description?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mb-4 flex flex-wrap items-end justify-between gap-3">
      <div className="min-w-0">
        <h1 className="text-lg font-semibold leading-7">{title}</h1>
        {description ? <p className="mt-0.5 text-xs text-muted-foreground">{description}</p> : null}
      </div>
      {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
    </div>
  );
}

export function Card({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('rounded-lg border bg-card', className)} {...props} />;
}

export function CardHeader({ title, actions, className }: { title: ReactNode; actions?: ReactNode; className?: string }) {
  return (
    <div className={cn('flex min-h-11 items-center justify-between gap-3 border-b px-4 py-2', className)}>
      <h2 className="text-sm font-semibold">{title}</h2>
      {actions ? <div className="flex items-center gap-2">{actions}</div> : null}
    </div>
  );
}

/** 筛选栏容器。 */
export function Toolbar({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn('flex flex-wrap items-center gap-2 border-b px-3 py-2.5', className)}>{children}</div>;
}

export function Spinner({ className }: { className?: string }) {
  return <LoaderCircle className={cn('size-4 animate-spin text-muted-foreground', className)} />;
}

export function LoadingBlock({ text = '加载中…' }: { text?: string }) {
  return (
    <div className="flex items-center justify-center gap-2 py-16 text-sm text-muted-foreground">
      <Spinner />
      {text}
    </div>
  );
}

export function ErrorBlock({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 py-16 text-sm">
      <span className="text-danger">加载失败：{errorMessage(error)}</span>
      {onRetry ? (
        <Button size="sm" onClick={onRetry}>
          重试
        </Button>
      ) : null}
    </div>
  );
}

export function Notice({
  tone = 'info',
  children,
  className,
}: {
  tone?: 'info' | 'warning' | 'danger';
  children: ReactNode;
  className?: string;
}) {
  const Icon = tone === 'info' ? Info : TriangleAlert;
  return (
    <div
      className={cn(
        'flex items-start gap-2 rounded-md border px-3 py-2 text-xs leading-5',
        tone === 'info' && 'border-info/30 bg-info/5 text-foreground',
        tone === 'warning' && 'border-warning/40 bg-warning/10 text-foreground',
        tone === 'danger' && 'border-danger/40 bg-danger/10 text-foreground',
        className,
      )}
    >
      <Icon
        className={cn(
          'mt-0.5 size-3.5 shrink-0',
          tone === 'info' && 'text-info',
          tone === 'warning' && 'text-warning',
          tone === 'danger' && 'text-danger',
        )}
      />
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}

export function FormError({ children }: { children: ReactNode }) {
  if (!children) return null;
  return (
    <div role="alert" className="rounded-md border border-danger/30 bg-danger/5 px-3 py-2 text-xs text-danger">
      {children}
    </div>
  );
}

export function CopyButton({ text, label = '复制', iconOnly = false }: { text: string; label?: string; iconOnly?: boolean }) {
  const [done, setDone] = useState(false);
  const toast = useToast();
  const onClick = async () => {
    if (await copyText(text)) {
      setDone(true);
      setTimeout(() => setDone(false), 1500);
    } else {
      toast.error('复制失败，请手动选择复制');
    }
  };
  return (
    <Button size={iconOnly ? 'icon-sm' : 'sm'} variant={iconOnly ? 'ghost' : 'secondary'} onClick={onClick} aria-label={label} title={label}>
      {done ? <Check className="text-success" /> : <Copy />}
      {iconOnly ? null : done ? '已复制' : label}
    </Button>
  );
}

export function QuotaBar({ label, percent, title }: { label: string; percent: number; title?: string }) {
  const p = Math.max(0, Math.min(100, percent));
  const tone = p >= 90 ? 'bg-danger' : p >= 70 ? 'bg-warning' : 'bg-success';
  return (
    <div className="flex items-center gap-1.5 text-xs" title={title}>
      <span className="w-8 shrink-0 text-muted-foreground">{label}</span>
      <div className="h-1.5 w-16 overflow-hidden rounded-full bg-muted">
        <div className={cn('h-full rounded-full', tone)} style={{ width: `${p}%` }} />
      </div>
      <span className="tabular w-9 text-right">{formatPercent(p)}</span>
    </div>
  );
}

export function SegmentedControl<T extends string>({
  value,
  onChange,
  options,
  'aria-label': ariaLabel,
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: ReactNode }[];
  'aria-label'?: string;
}) {
  return (
    <div role="radiogroup" aria-label={ariaLabel} className="inline-flex rounded-md border border-input bg-muted/40 p-0.5">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={cn(
            'rounded px-2 py-0.5 text-xs text-muted-foreground transition-colors hover:text-foreground',
            value === o.value && 'bg-card font-medium text-foreground shadow-sm',
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function JsonBlock({ value, className }: { value: unknown; className?: string }) {
  let text: string;
  try {
    text = value === undefined ? '（无）' : JSON.stringify(value, null, 2);
  } catch {
    text = String(value);
  }
  return (
    <pre className={cn('max-h-80 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-xs leading-5', className)}>
      {text}
    </pre>
  );
}

/** 详情面板里的「标签：值」网格。 */
export function DescList({ items, className }: { items: { label: ReactNode; value: ReactNode }[]; className?: string }) {
  return (
    <dl className={cn('grid grid-cols-[7rem_1fr] gap-x-3 gap-y-2 text-sm', className)}>
      {items.map((it, i) => (
        <div key={i} className="contents">
          <dt className="text-xs leading-5 text-muted-foreground">{it.label}</dt>
          <dd className="min-w-0 break-words leading-5">{it.value}</dd>
        </div>
      ))}
    </dl>
  );
}
