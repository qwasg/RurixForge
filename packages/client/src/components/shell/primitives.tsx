import { useCallback, useState, type ReactNode } from 'react';
import { Maximize2, Minimize2, PanelLeft, PanelRight } from 'lucide-react';
import { cn } from '@/lib/cn';
import { PANE_CLAMP, useWorkbenchStore, type PaneKind } from '@/lib/workbenchStore';

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

/** 栏头图标钮底样(折叠 / 缩小共用) */
const PANE_BTN =
  'flex shrink-0 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2';

/** 面板折叠钮(侧栏图标,替代 9px 分隔条胶囊) */
export function PaneToggleBtn({ kind, className }: { kind: PaneKind; className?: string }) {
  const togglePane = useWorkbenchStore((st) => st.togglePane);
  const Icon = kind === 'inspector' ? PanelRight : PanelLeft;
  return (
    <button
      type="button"
      title="折叠"
      aria-label={`折叠 ${kind}`}
      data-testid={`pane-toggle-${kind}`}
      onClick={() => togglePane(kind)}
      className={cn(PANE_BTN, className)}
    >
      <Icon size={14} strokeWidth={1.75} />
    </button>
  );
}

/**
 * 对话缩小/还原钮(2026-08-25 用户拍板,坐在对话列头折叠钮左侧):
 * 缩小 → 对话收成主区左下角浮窗 + 会话栏一并收起;再点还原回三栏对话列。
 */
export function ChatMiniBtn({ className }: { className?: string }) {
  const chatMini = useWorkbenchStore((st) => st.chatMini);
  const setChatMini = useWorkbenchStore((st) => st.setChatMini);
  const label = chatMini ? '还原对话窗口' : '缩小对话窗口';
  const Icon = chatMini ? Maximize2 : Minimize2;
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      data-testid="chat-mini-toggle"
      onClick={() => setChatMini(!chatMini)}
      className={cn(PANE_BTN, className)}
    >
      <Icon size={13} strokeWidth={1.75} />
    </button>
  );
}

/**
 * 栏宽拖拽条(2026-08-25 用户拍板「四个区的分界线要能自由拉」):
 * 骑在 1px 分栏线上的 5px 透明热区,绝对定位不占位——拉的是相邻定宽栏,
 * 主区吃剩下的宽度,所以三条线覆盖会话栏/对话列/主区/右栏四个区。
 * 宽度 clamp 在 store 里(PANE_CLAMP),双击回默认宽。
 * side = 热区贴哪侧:定宽栏在左(会话栏/对话列)贴右缘,在右(右栏)贴左缘,拖动方向随之取反。
 */
export function PaneResizer({ kind, side }: { kind: PaneKind; side: 'left' | 'right' }) {
  const setPaneW = useWorkbenchStore((st) => st.setPaneW);
  const [dragging, setDragging] = useState(false);

  const onDragStart = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault(); // 顺带压掉拖拽期间的选中高亮
      const startX = e.clientX;
      const startW = useWorkbenchStore.getState().paneW[kind];
      const dir = side === 'right' ? 1 : -1;
      setDragging(true);
      const onMove = (ev: MouseEvent) => setPaneW(kind, startW + dir * (ev.clientX - startX));
      const onUp = () => {
        setDragging(false);
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
      };
      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    },
    [kind, setPaneW, side],
  );

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={`拖动改栏宽 ${kind}`}
      title="拖动改栏宽,双击回默认宽"
      data-testid={`pane-resize-${kind}`}
      onMouseDown={onDragStart}
      onDoubleClick={() => setPaneW(kind, PANE_CLAMP[kind].def)}
      className={cn(
        'absolute top-0 z-20 h-full w-[5px] cursor-col-resize bg-transparent transition-colors hover:bg-acc-ring',
        side === 'right' ? '-right-[2px]' : '-left-[2px]',
        dragging && 'bg-acc-ring',
      )}
    />
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
