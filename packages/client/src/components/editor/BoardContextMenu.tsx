import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { LucideIcon } from 'lucide-react';
import { cn } from '@/lib/cn';

/**
 * 画板右键浮层(画布 / 实体 / 特性三套共用同一外壳)。
 * 三套菜单的项完全不同,靠标题行(图标 + 上下文名 + 作用域文字)一眼分清右键点在了谁身上:
 * 点空白 = 「画布」,点卡身 = 「<实体名> · <类型>」,点特性子节点 = 「<组件名> · 特性」。
 * 每项右侧标注等效键盘快捷键,菜单与快捷键走同一批回调,行为不会各走一套。
 * 定位用 fixed + 屏幕坐标,贴到视口右/下边时向反侧翻,不被裁掉。
 */

export interface BoardMenuAction {
  key: string;
  label: string;
  /** 等效键盘快捷键(只作展示,真正的绑定在画布上) */
  keys?: string;
  hint?: string;
  icon?: LucideIcon;
  /** 图标着色类(如类型色),缺省跟随文字 */
  iconClass?: string;
  danger?: boolean;
  disabled?: boolean;
  onSelect: () => void;
}

/** 分隔线:用 { sep: true } 与动作项区分 */
export type BoardMenuItem = BoardMenuAction | { sep: true; key: string };

function isSep(item: BoardMenuItem): item is { sep: true; key: string } {
  return 'sep' in item;
}

export interface BoardContextMenuProps {
  /** 右键点位(client 坐标) */
  x: number;
  y: number;
  /** 标题行:作用域名称 + 副标题,右键点到谁身上一目了然 */
  title: string;
  scope: string;
  icon: LucideIcon;
  /** 标题行图标与左侧色条的着色类(实体用类型色,特性用 warn,画布用中性) */
  accentClass?: string;
  items: BoardMenuItem[];
  onClose: () => void;
  testid: string;
}

const MENU_W = 216;
/** 贴边翻转的余量 */
const EDGE_PAD = 8;

export default function BoardContextMenu({
  x,
  y,
  title,
  scope,
  icon: Icon,
  accentClass = 'text-fg-3',
  items,
  onClose,
  testid,
}: BoardContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });

  // 贴到视口右/下边就向反侧翻(菜单高度要等挂载后量,故用 layout effect 在绘制前定位)
  useLayoutEffect(() => {
    const h = ref.current?.offsetHeight ?? 0;
    const vw = globalThis.innerWidth || 0;
    const vh = globalThis.innerHeight || 0;
    const left = vw > 0 && x + MENU_W > vw - EDGE_PAD ? Math.max(EDGE_PAD, x - MENU_W) : x;
    const top = vh > 0 && h > 0 && y + h > vh - EDGE_PAD ? Math.max(EDGE_PAD, y - h) : y;
    setPos({ left, top });
  }, [x, y, items.length]);

  // 点外部 / Esc / 滚轮关闭:捕获期拿事件,免得被卡片自己的拖拽与端口抢走
  useEffect(() => {
    const onDown = (ev: MouseEvent) => {
      if (!ref.current?.contains(ev.target as Node)) onClose();
    };
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === 'Escape') {
        ev.stopPropagation(); // 不连带触发 Shell 的全局 Esc(会关掉整个 overlay)
        onClose();
      }
    };
    document.addEventListener('mousedown', onDown, true);
    document.addEventListener('keydown', onKey, true);
    globalThis.addEventListener('wheel', onClose, { passive: true });
    return () => {
      document.removeEventListener('mousedown', onDown, true);
      document.removeEventListener('keydown', onKey, true);
      globalThis.removeEventListener('wheel', onClose);
    };
  }, [onClose]);

  return (
    <div
      ref={ref}
      role="menu"
      data-testid={testid}
      style={{ left: pos.left, top: pos.top, width: MENU_W }}
      className="fixed z-50 overflow-hidden rounded-md border border-edge-strong bg-shell-float py-1 shadow-pop"
      onContextMenu={(e) => e.preventDefault()}
    >
      <div className="flex items-center gap-1 px-2 pb-1">
        <Icon size={11} strokeWidth={1.8} className={cn('shrink-0', accentClass)} />
        <span className="min-w-0 flex-1 truncate text-2xs font-medium text-fg" title={title}>
          {title}
        </span>
        <span className="shrink-0 text-[10px] text-fg-4">{scope}</span>
      </div>
      <div className="mb-1 h-px bg-edge" />
      {items.map((item) =>
        isSep(item) ? (
          <div key={item.key} className="my-1 h-px bg-edge" />
        ) : (
          <button
            key={item.key}
            type="button"
            role="menuitem"
            data-testid={`${testid}-${item.key}`}
            disabled={item.disabled}
            title={item.hint}
            onClick={() => {
              item.onSelect();
              onClose();
            }}
            className={cn(
              'flex w-full items-center gap-1.5 px-2 py-1 text-left text-2xs transition-colors',
              item.disabled
                ? 'cursor-not-allowed text-fg-4'
                : item.danger
                  ? 'text-fg-2 hover:bg-danger/10 hover:text-danger'
                  : 'text-fg-2 hover:bg-shell-hover hover:text-fg',
            )}
          >
            {item.icon && (
              <item.icon
                size={11}
                strokeWidth={1.8}
                className={cn('shrink-0', item.iconClass ?? 'text-fg-4')}
              />
            )}
            <span className="min-w-0 flex-1 truncate">{item.label}</span>
            {item.keys && (
              <span className="shrink-0 font-mono text-[10px] text-fg-4">{item.keys}</span>
            )}
          </button>
        ),
      )}
    </div>
  );
}
