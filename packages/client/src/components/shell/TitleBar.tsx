import { useEffect, useRef, useState } from 'react';
import {
  MessageSquareText,
  Minus,
  PanelBottom,
  PanelLeft,
  PanelRight,
  Search,
  Square,
  SquareSquare,
  X,
} from 'lucide-react';
import { bridge, isDesktopBridge } from '@/lib/bridge';
import { cn } from '@/lib/cn';
import { COMMANDS, runCommand } from '@/lib/commands';
import { KEYS } from '@/lib/shortcuts';
import { applyPalette, useThemeStore } from '@/lib/themeStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import ForgeLogo from '@/components/ForgeLogo';
import { Kbd, MenuItem, MenuSep } from './primitives';

/**
 * TitleBar(36px):左侧项目 logo + File/Edit/View/Help 菜单;
 * 中央 max-w-520 搜索胶囊(开命令面板,搜会话/文件/命令);右侧面板开关组(Cursor 布局)。
 *
 * 窗口按钮按 bridge().win.chrome 分三种形态:
 * - overlay(Windows/Linux 桌面端):系统经 titleBarOverlay 绘制三钮(保留贴靠布局),
 *   这里只按 Window Controls Overlay 的 env(titlebar-area-*) 让出宽度,并把主题色同步给系统;
 * - inset(macOS):左侧给红绿灯让位;
 * - 缺省:旧桌面壳自绘三钮;纯浏览器不渲染(不伪造)。
 */

type MenuKey = 'file' | 'edit' | 'view' | 'help';

interface MenuRow {
  label: string;
  shortcut?: string;
  command?: string;
  action?: () => void;
  sep?: boolean;
  /** 仅桌面端呈现的菜单项(纯浏览器环境隐藏,不伪造不可用入口) */
  desktopOnly?: boolean;
}

function execCmd(name: string): void {
  // Edit 菜单:复制/剪切/粘贴/全选(web 走 execCommand)
  document.execCommand(name);
}

const MENUS: Record<MenuKey, { label: string; rows: MenuRow[] }> = {
  file: {
    label: 'File',
    rows: [
      { label: '新建会话', command: 'session.new' },
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
      { label: '缩小/还原对话窗口', command: 'pane.chatMini' },
      { label: '切换 Inspector', command: 'pane.inspector' },
      { label: '切换底部面板', command: 'bottom.toggle' },
      { sep: true, label: '' },
      { label: '切换主题（浅色/深色）', command: 'theme.toggle' },
      { label: '命令面板', command: 'palette.open' },
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

/** 过滤桌面专有菜单项,并裁掉过滤后悬空的首尾分隔线(浏览器环境不留孤儿分隔线)。 */
function visibleRows(rows: MenuRow[], desktop: boolean): MenuRow[] {
  const filtered = rows.filter((r) => desktop || !r.desktopOnly);
  let start = 0;
  let end = filtered.length;
  while (start < end && filtered[start].sep) start++;
  while (end > start && filtered[end - 1].sep) end--;
  return filtered.slice(start, end);
}

/** 菜单行快捷键:显式声明优先,否则取命令注册表(与命令面板同一事实源)。 */
function rowShortcut(row: MenuRow): string | undefined {
  if (row.shortcut) return row.shortcut;
  return row.command ? COMMANDS.find((c) => c.id === row.command)?.shortcut : undefined;
}

/** overlay 形态:系统三钮占去的宽度 = 视口宽 − 标题栏可用区右缘(env 缺省兜底 138 = 3×46)。 */
const OVERLAY_INSET = 'calc(100vw - env(titlebar-area-x, 0px) - env(titlebar-area-width, calc(100vw - 138px)))';

export default function TitleBar() {
  const [openMenu, setOpenMenu] = useState<MenuKey | null>(null);
  const [maximized, setMaximized] = useState(false);
  const rootRef = useRef<HTMLElement>(null);
  const desktop = isDesktopBridge();
  const chrome = desktop ? bridge().win.chrome : undefined;
  const collapsed = useWorkbenchStore((st) => st.collapsed);
  const togglePane = useWorkbenchStore((st) => st.togglePane);
  const bottomOpen = useWorkbenchStore((st) => st.bottomOpen);
  const toggleBottom = useWorkbenchStore((st) => st.toggleBottom);
  const isDark = useThemeStore((st) => st.isDark);
  const lightPalette = useThemeStore((st) => st.light);
  const darkPalette = useThemeStore((st) => st.dark);

  const paneBtn = (on: boolean) =>
    cn(
      'flex h-6 w-7 items-center justify-center rounded text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg',
      on && 'text-fg-2',
    );

  useEffect(() => {
    const off = bridge().win.onMaximizedChanged(setMaximized);
    return () => off();
  }, []);

  // overlay 形态:系统三钮跟标题栏同底色(--bg-sunk)与次级字色(--text-2),随主题/预设实时换
  useEffect(() => {
    if (chrome !== 'overlay') return;
    const setTheme = bridge().win.setOverlayTheme;
    if (!setTheme) return;
    const tokens = applyPalette(isDark ? darkPalette : lightPalette, isDark);
    setTheme({ color: tokens['bg-sunk'], symbolColor: tokens['text-2'] });
  }, [chrome, isDark, lightPalette, darkPalette]);

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
      data-chrome={chrome ?? (desktop ? 'custom' : 'web')}
      className={cn(
        'relative flex h-[36px] shrink-0 items-center gap-2 border-b border-edge bg-shell-sunk px-2.5 [-webkit-app-region:drag]',
        chrome === 'inset' && 'pl-[78px]',
        chrome === 'overlay' && 'pr-0',
      )}
    >
      {/* 左:logo + 菜单栏 */}
      <div className="flex items-center gap-2 [-webkit-app-region:no-drag]">
        <ForgeLogo className="h-[22px] w-[22px] rounded-[6px]" />
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
                        shortcut={rowShortcut(row)}
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

      {/* 中央:搜索胶囊(命令面板:会话 / 文件 / 命令) */}
      <div className="flex min-w-0 flex-1 items-center justify-center">
        <button
          type="button"
          data-testid="titlebar-search"
          onClick={() => runCommand('palette.open')}
          className="flex h-6 w-full max-w-[520px] items-center gap-1.5 rounded-full border border-edge bg-shell-panel px-2.5 text-left transition-colors hover:border-edge-strong [-webkit-app-region:no-drag]"
        >
          <Search size={12} className="shrink-0 text-fg-3" />
          <span className="min-w-0 flex-1 truncate text-[12px] text-fg-3">搜索会话、文件、命令…</span>
          <Kbd label={KEYS.palette} />
        </button>
      </div>

      {/* 右:面板开关组(会话栏 / 对话栏 / 底部面板 / 右栏) */}
      <div className="flex items-center gap-0.5 [-webkit-app-region:no-drag]">
        <button
          type="button"
          title={`切换会话栏(${KEYS.toggleSessions})`}
          aria-label="切换会话栏"
          aria-pressed={!collapsed.sessions}
          className={paneBtn(!collapsed.sessions)}
          onClick={() => togglePane('sessions')}
        >
          <PanelLeft size={14} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          title="切换对话栏"
          aria-label="切换对话栏"
          aria-pressed={!collapsed.chat}
          className={paneBtn(!collapsed.chat)}
          onClick={() => togglePane('chat')}
        >
          <MessageSquareText size={14} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          title={`切换底部面板(${KEYS.toggleBottom})`}
          aria-label="切换底部面板"
          aria-pressed={bottomOpen}
          data-testid="titlebar-bottom-toggle"
          className={paneBtn(bottomOpen)}
          onClick={toggleBottom}
        >
          <PanelBottom size={14} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          title={`切换右栏(${KEYS.toggleInspector})`}
          aria-label="切换 Inspector"
          aria-pressed={!collapsed.inspector}
          className={paneBtn(!collapsed.inspector)}
          onClick={() => togglePane('inspector')}
        >
          <PanelRight size={14} strokeWidth={1.75} />
        </button>
      </div>

      {/* overlay:给系统三钮让位(不渲染按钮);旧桌面壳:自绘三钮;浏览器:不渲染 */}
      {chrome === 'overlay' && (
        <span aria-hidden data-testid="titlebar-overlay-inset" className="h-full shrink-0" style={{ width: OVERLAY_INSET }} />
      )}
      {desktop && chrome === undefined && (
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
