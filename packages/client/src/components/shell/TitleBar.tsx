import { useEffect, useRef, useState } from 'react';
import { Minus, Search, Square, SquareSquare, X } from 'lucide-react';
import { bridge, isDesktopBridge } from '@/lib/bridge';
import { cn } from '@/lib/cn';
import { runCommand } from '@/lib/commands';
import { Kbd, MenuItem, MenuSep } from './primitives';

/**
 * F7 wave.3 TitleBar(36px,参考 ui/titlebar.rs):
 * 左 logo 方块(serif「铸」,22×22 圆角 6,bg=text 字色 text_inv)+ File/Edit/View/Help 菜单
 * (26px 行高 + kbd 提示,玻璃下拉);中央 max-w-520 搜索胶囊(点击开命令面板);
 * 右 Windows 三钮(minimize/toggleMaximize/close,最大化态订阅,close hover 红底)。
 */

type MenuKey = 'file' | 'edit' | 'view' | 'help';

interface MenuRow {
  label: string;
  shortcut?: string;
  command?: string;
  action?: () => void;
  sep?: boolean;
  /** F8 wave.3:仅桌面端呈现的菜单项(纯浏览器环境隐藏,不伪造不可用入口) */
  desktopOnly?: boolean;
}

function execCmd(name: string): void {
  // Edit 菜单:复制/剪切/粘贴/全选(参考走 gpui action;web 走 execCommand)
  document.execCommand(name);
}

const MENUS: Record<MenuKey, { label: string; rows: MenuRow[] }> = {
  file: {
    label: 'File',
    rows: [
      { label: '新建会话', shortcut: 'Ctrl+Shift+N', command: 'session.new' },
      { label: '打开编辑器', command: 'tab.editor' },
      { sep: true, label: '' },
      { label: '设置', command: 'settings.open' },
      { sep: true, label: '' },
      { label: '退出', action: () => bridge().win.close(), desktopOnly: true },
    ],
  },
  edit: {
    label: 'Edit',
    rows: [
      { label: '复制', shortcut: 'Ctrl+C', action: () => execCmd('copy') },
      { label: '剪切', shortcut: 'Ctrl+X', action: () => execCmd('cut') },
      { label: '粘贴', shortcut: 'Ctrl+V', action: () => execCmd('paste') },
      { sep: true, label: '' },
      { label: '全选', shortcut: 'Ctrl+A', action: () => execCmd('selectAll') },
    ],
  },
  view: {
    label: 'View',
    rows: [
      { label: '切换会话栏', command: 'pane.sessions' },
      { label: '切换对话栏', command: 'pane.chat' },
      { label: '切换 Inspector', command: 'pane.inspector' },
      { sep: true, label: '' },
      { label: '切换主题（浅色/深色）', command: 'theme.toggle' },
      { label: '命令面板', shortcut: 'Ctrl+K', command: 'palette.open' },
    ],
  },
  help: {
    label: 'Help',
    rows: [
      { label: '键盘快捷键', command: 'help.shortcuts' },
      { sep: true, label: '' },
      { label: '关于 RurixForge', command: 'help.about' },
    ],
  },
};

const MENU_KEYS: MenuKey[] = ['file', 'edit', 'view', 'help'];

/** F8 wave.3:过滤桌面专有菜单项,并裁掉过滤后悬空的首尾分隔线(浏览器环境不留孤儿分隔线)。 */
function visibleRows(rows: MenuRow[], desktop: boolean): MenuRow[] {
  const filtered = rows.filter((r) => desktop || !r.desktopOnly);
  let start = 0;
  let end = filtered.length;
  while (start < end && filtered[start].sep) start++;
  while (end > start && filtered[end - 1].sep) end--;
  return filtered.slice(start, end);
}

export default function TitleBar() {
  const [openMenu, setOpenMenu] = useState<MenuKey | null>(null);
  const [maximized, setMaximized] = useState(false);
  const rootRef = useRef<HTMLElement>(null);
  const desktop = isDesktopBridge();

  useEffect(() => {
    const off = bridge().win.onMaximizedChanged(setMaximized);
    return () => off();
  }, []);

  // Esc / 点击外部 收起菜单
  useEffect(() => {
    if (!openMenu) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpenMenu(null);
    };
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpenMenu(null);
    };
    window.addEventListener('keydown', onKey);
    document.addEventListener('mousedown', onDown);
    return () => {
      window.removeEventListener('keydown', onKey);
      document.removeEventListener('mousedown', onDown);
    };
  }, [openMenu]);

  return (
    <header
      ref={rootRef}
      data-testid="shell-titlebar"
      className="relative flex h-[36px] shrink-0 items-center gap-2 border-b border-edge bg-shell-sunk px-2.5 [-webkit-app-region:drag]"
    >
      {/* 左:logo + 菜单栏 */}
      <div className="flex items-center gap-2 [-webkit-app-region:no-drag]">
        <span
          className="flex h-[22px] w-[22px] shrink-0 select-none items-center justify-center rounded-[6px] bg-fg font-serif text-[14px] text-fg-inv"
          aria-label="RurixForge"
        >
          铸
        </span>
        <nav className="flex items-center">
          {MENU_KEYS.map((key) => (
            <div key={key} className="relative">
              <button
                type="button"
                data-testid={`menu-${key}`}
                onClick={() => setOpenMenu(openMenu === key ? null : key)}
                onMouseEnter={() => {
                  if (openMenu && openMenu !== key) setOpenMenu(key);
                }}
                className={cn(
                  'flex h-6 items-center rounded px-2 text-[12px] text-fg-2 transition-colors hover:bg-shell-hover',
                  openMenu === key && 'bg-shell-active',
                )}
              >
                {MENUS[key].label}
              </button>
              {openMenu === key && (
                <div
                  role="menu"
                  className="absolute left-0 top-full z-50 min-w-[240px] rounded-b-lg rounded-t-none border border-t-0 border-edge-strong p-1 pb-1 shadow-float backdrop-blur"
                  style={{ background: 'var(--menu-glass)' }}
                >
                  {visibleRows(MENUS[key].rows, desktop).map((row, i) =>
                    row.sep ? (
                      <MenuSep key={i} />
                    ) : (
                      <MenuItem
                        key={row.label}
                        label={row.label}
                        shortcut={row.shortcut}
                        onSelect={() => {
                          setOpenMenu(null);
                          if (row.command) runCommand(row.command);
                          else row.action?.();
                        }}
                      />
                    ),
                  )}
                </div>
              )}
            </div>
          ))}
        </nav>
      </div>

      {/* 中央:搜索胶囊(点击开命令面板) */}
      <div className="flex min-w-0 flex-1 items-center justify-center">
        <button
          type="button"
          data-testid="titlebar-search"
          onClick={() => runCommand('palette.open')}
          className="flex h-6 w-full max-w-[520px] items-center gap-1.5 rounded-full border border-edge bg-shell-panel px-2.5 text-left transition-colors hover:border-edge-strong [-webkit-app-region:no-drag]"
        >
          <Search size={12} className="shrink-0 text-fg-3" />
          <span className="min-w-0 flex-1 truncate text-[12px] text-fg-3">搜索会话、文件、命令…</span>
          <Kbd label="Ctrl K" />
        </button>
      </div>

      {/* 右:Windows 窗口三钮(仅桌面端渲染;浏览器环境隐藏,不伪造) */}
      {desktop && (
        <div className="flex items-stretch self-stretch [-webkit-app-region:no-drag]">
          <button
            type="button"
            title="Minimize"
            aria-label="Minimize"
            onClick={() => bridge().win.minimize()}
            className="flex w-[46px] items-center justify-center text-fg-2 transition-colors hover:bg-shell-hover"
          >
            <Minus size={13} strokeWidth={1.5} />
          </button>
          <button
            type="button"
            title={maximized ? 'Restore' : 'Maximize'}
            aria-label={maximized ? 'Restore' : 'Maximize'}
            onClick={() => bridge().win.toggleMaximize()}
            className="flex w-[46px] items-center justify-center text-fg-2 transition-colors hover:bg-shell-hover"
          >
            {maximized ? (
              <SquareSquare size={12} strokeWidth={1.5} />
            ) : (
              <Square size={12} strokeWidth={1.5} />
            )}
          </button>
          <button
            type="button"
            title="Close"
            aria-label="Close"
            onClick={() => bridge().win.close()}
            className="flex w-[46px] items-center justify-center text-fg-2 transition-colors hover:bg-danger hover:text-fg-inv"
          >
            <X size={14} strokeWidth={1.5} />
          </button>
        </div>
      )}
    </header>
  );
}
