import { cn } from '@/lib/cn';
import { normalizeTodoStatus, type TodoStatus } from '@/lib/todoStatus';

const STATUS: Record<TodoStatus, { label: string; color: string }> = {
  pending: { label: '待办', color: 'var(--dot-queued)' },
  running: { label: '制作中', color: 'var(--dot-running)' },
  completed: { label: '完成', color: 'var(--dot-done)' },
  failed: { label: '失败', color: 'var(--dot-blocked)' },
};

/** 四态圆圈随任务状态更新；以轮廓与中心符号区分，颜色沿用当前主题。 */
export default function TodoStatusIndicator({ status, className }: { status: string; className?: string }) {
  const state = normalizeTodoStatus(status);
  const { label, color } = STATUS[state];
  return (
    <svg
      role="img"
      aria-label={label}
      data-todo-status={state}
      viewBox="0 0 16 16"
      width={16}
      height={16}
      fill="none"
      className={cn('h-4 w-4 shrink-0', className)}
      style={{ color, opacity: state === 'pending' ? 0.45 : 1 }}
    >
      <title>{label}</title>
      <circle
        cx={8}
        cy={8}
        r={6.75}
        stroke="currentColor"
        strokeWidth={1.25}
        strokeDasharray={state === 'pending' ? undefined : '2 2'}
      />
      {state === 'running' && <circle cx={8} cy={8} r={4.25} fill="currentColor" />}
      {state === 'completed' && (
        <path d="M4.5 8 6.75 10.25 11.5 5.75" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" strokeLinejoin="round" />
      )}
      {state === 'failed' && (
        <path d="M5.5 10.5 10.5 5.5" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" />
      )}
    </svg>
  );
}
