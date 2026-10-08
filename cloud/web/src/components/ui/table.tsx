import { ChevronLeft, ChevronRight, LoaderCircle } from 'lucide-react';
import type { HTMLAttributes, ReactNode, TdHTMLAttributes, ThHTMLAttributes } from 'react';
import { errorMessage } from '@/lib/api/client';
import { formatNumber } from '@/lib/format';
import { cn } from '@/lib/utils';
import { Button } from './button';

export function Table({ className, children, ...props }: HTMLAttributes<HTMLTableElement>) {
  return (
    <div className="overflow-x-auto">
      <table className={cn('w-full border-collapse text-sm', className)} {...props}>
        {children}
      </table>
    </div>
  );
}

export function THead({ className, ...props }: HTMLAttributes<HTMLTableSectionElement>) {
  return <thead className={cn('bg-muted/50 text-xs text-muted-foreground', className)} {...props} />;
}

export function TBody(props: HTMLAttributes<HTMLTableSectionElement>) {
  return <tbody {...props} />;
}

export function TR({ className, ...props }: HTMLAttributes<HTMLTableRowElement>) {
  return <tr className={cn('transition-colors hover:bg-muted/40', className)} {...props} />;
}

export function TH({ className, ...props }: ThHTMLAttributes<HTMLTableCellElement>) {
  return <th className={cn('whitespace-nowrap border-b px-3 py-2 text-left font-medium', className)} {...props} />;
}

export function TD({ className, ...props }: TdHTMLAttributes<HTMLTableCellElement>) {
  return <td className={cn('border-b px-3 py-2 align-middle', className)} {...props} />;
}

/**
 * 表格占位行：首次加载 / 出错 / 空。已有数据时的重新加载不显示（保留旧数据）。
 * 返回 null 表示应渲染数据行。
 */
export function TableStatus({
  colSpan,
  loading,
  error,
  empty,
  emptyText = '暂无数据',
  onRetry,
}: {
  colSpan: number;
  loading: boolean;
  error: unknown;
  empty: boolean;
  emptyText?: ReactNode;
  onRetry?: () => void;
}) {
  let content: ReactNode = null;
  if (error && empty) {
    content = (
      <div className="flex flex-col items-center gap-2">
        <span className="text-danger">加载失败：{errorMessage(error)}</span>
        {onRetry ? (
          <Button size="sm" onClick={onRetry}>
            重试
          </Button>
        ) : null}
      </div>
    );
  } else if (loading && empty) {
    content = (
      <span className="inline-flex items-center gap-2">
        <LoaderCircle className="size-4 animate-spin" />
        加载中…
      </span>
    );
  } else if (empty) {
    content = emptyText;
  }
  if (content === null) return null;
  return (
    <tr>
      <td colSpan={colSpan} className="border-b px-3 py-10 text-center text-sm text-muted-foreground">
        {content}
      </td>
    </tr>
  );
}

const PAGE_SIZES = [20, 50, 100, 200];

export function Pagination({
  total,
  limit,
  offset,
  onOffsetChange,
  onLimitChange,
  className,
}: {
  total: number;
  limit: number;
  offset: number;
  onOffsetChange: (offset: number) => void;
  onLimitChange?: (limit: number) => void;
  className?: string;
}) {
  const page = Math.floor(offset / limit) + 1;
  const pages = Math.max(1, Math.ceil(total / limit));
  return (
    <div className={cn('flex flex-wrap items-center justify-between gap-3 px-3 py-2 text-xs text-muted-foreground', className)}>
      <span>共 {formatNumber(total)} 条</span>
      <div className="flex items-center gap-1.5">
        {onLimitChange ? (
          <select
            aria-label="每页条数"
            className="h-7 rounded-md border border-input bg-card px-1.5 text-xs text-foreground"
            value={limit}
            onChange={(e) => onLimitChange(Number(e.target.value))}
          >
            {PAGE_SIZES.map((n) => (
              <option key={n} value={n}>
                {n} 条/页
              </option>
            ))}
          </select>
        ) : null}
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="上一页"
          disabled={page <= 1}
          onClick={() => onOffsetChange(Math.max(0, offset - limit))}
        >
          <ChevronLeft />
        </Button>
        <span className="tabular min-w-12 text-center text-foreground">
          {page} / {pages}
        </span>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="下一页"
          disabled={page >= pages}
          onClick={() => onOffsetChange(offset + limit)}
        >
          <ChevronRight />
        </Button>
      </div>
    </div>
  );
}
