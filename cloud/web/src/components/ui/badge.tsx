import { cva, type VariantProps } from 'class-variance-authority';
import type { HTMLAttributes, ReactNode } from 'react';
import { cn } from '@/lib/utils';

const badgeVariants = cva(
  'inline-flex items-center gap-1 whitespace-nowrap rounded px-1.5 py-px text-xs font-medium leading-4 [&_svg]:size-3',
  {
    variants: {
      tone: {
        neutral: 'bg-muted text-muted-foreground',
        success: 'bg-success/10 text-success',
        warning: 'bg-warning/10 text-warning',
        danger: 'bg-danger/10 text-danger',
        info: 'bg-info/10 text-info',
        outline: 'border border-border text-muted-foreground',
        solid: 'bg-primary text-primary-foreground',
      },
    },
    defaultVariants: { tone: 'neutral' },
  },
);

export type BadgeTone = NonNullable<VariantProps<typeof badgeVariants>['tone']>;

export function Badge({
  tone,
  className,
  ...props
}: HTMLAttributes<HTMLSpanElement> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ tone }), className)} {...props} />;
}

/** 带圆点的状态标记。 */
export function StatusDot({ tone, children }: { tone: BadgeTone; children: ReactNode }) {
  const dot: Record<BadgeTone, string> = {
    neutral: 'bg-muted-foreground',
    success: 'bg-success',
    warning: 'bg-warning',
    danger: 'bg-danger',
    info: 'bg-info',
    outline: 'bg-muted-foreground',
    solid: 'bg-primary',
  };
  return (
    <span className="inline-flex items-center gap-1.5 whitespace-nowrap text-xs">
      <span className={cn('size-1.5 rounded-full', dot[tone])} />
      {children}
    </span>
  );
}
