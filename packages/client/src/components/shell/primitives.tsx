import type { ReactNode } from 'react';
import { cn } from '@/lib/cn';

/**
 * F7 wave.3 壳 primitives(参考 ui/mod.rs:status_dot/kbd/sec_head/ibtn/menu_item)。
 * 颜色全部走主题 CSS 变量(tailwind 语义 token 映射,见 tailwind.config.ts)。
 */

/** 6×6 状态点(参考 status_dot) */
export function StatusDot({ color, pulse }: { color: string; pulse?: boolean }) {
  return (
    <span
      className={cn('h-[6px] w-[6px] shrink-0 rounded-full', pulse && 'animate-pulse')}
      style={{ background: color }}
    />
  );
}

/** 16px 高 mono 键位提示(参考 .kbd) */
export function Kbd({ label }: { label: string }) {
  return (
    <span className="flex h-4 items-center rounded border border-edge bg-shell-sunk px-1 font-code text-[10px] text-fg-3">
      {label}
    </span>
  );
}

/** 10px 大写小节头(参考 .sec-head;uppercase 由调用方文案保证,兼容中文) */
export function SecHead({
  icon,
  label,
  children,
}: {
  icon?: ReactNode;
  label: string;
  children?: ReactNode;
}) {
  return (
    <div className="flex items-center gap-[5px] px-3 pb-1 pt-2.5 text-[10px] font-semibold tracking-wide text-fg-4">
      {icon}
      <span className="uppercase">{label}</span>
      {children}
    </div>
  );
}

/** 26×26 图标钮(参考 .ibtn) */
export function IBtn({
  title,
  onClick,
  children,
  active,
  testId,
}: {
  title: string;
  onClick: (e: React.MouseEvent) => void;
  children: ReactNode;
  active?: boolean;
  testId?: string;
}) {
  return (
    <button
      type="button"
      title={title}
      data-testid={testId}
      aria-label={title}
      onClick={(e) => {
        e.stopPropagation();
        onClick(e);
      }}
      className={cn(
        'flex h-[26px] w-[26px] shrink-0 items-center justify-center rounded-md text-fg-2 transition-colors hover:bg-shell-hover',
        active && 'bg-shell-active',
      )}
    >
      {children}
    </button>
  );
}

/** 26px 下拉行(参考 .menu-item:icon + label + 可选 kbd) */
export function MenuItem({
  icon,
  label,
  shortcut,
  onSelect,
}: {
  icon?: ReactNode;
  label: string;
  shortcut?: string;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={(e) => {
        e.stopPropagation();
        onSelect();
      }}
      className="flex h-[26px] w-full items-center gap-2 rounded-md px-2 text-left text-[12px] text-fg-2 transition-colors hover:bg-shell-selection hover:text-acc"
    >
      {icon && <span className="flex h-[13px] w-[13px] shrink-0 items-center justify-center text-fg-3">{icon}</span>}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {shortcut && <Kbd label={shortcut} />}
    </button>
  );
}

/** 菜单分隔线(参考 sep:h1 my4 bg line) */
export function MenuSep() {
  return <div className="my-1 h-px bg-shell-hover" style={{ background: 'var(--line)' }} />;
}
