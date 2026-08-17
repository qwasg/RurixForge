import { useEffect, useRef, useState } from 'react';
import {
  Box,
  ChevronLeft,
  ChevronRight,
  Copy,
  Minus,
  Square,
  SquareSquare,
  X,
  PanelLeft,
  PanelRight,
  ArrowUpRight,
  MoreHorizontal,
} from 'lucide-react';
import { useAppStore } from '@/lib/store';
import { bridge } from '@/lib/bridge';
import { AGENT_TITLE } from '@/lib/mock';
import { cn } from '@/lib/cn';

type MenuKey = 'file' | 'edit' | 'view' | 'help' | 'more';

type MenuItem = { label: string; shortcut?: string; onSelect: () => void } | 'sep';

/** 下拉菜单容器(白底圆角 + shadow-pop 浮层) */
function Dropdown({
  items,
  align = 'left',
  onClose,
}: {
  items: MenuItem[];
  align?: 'left' | 'right';
  onClose: () => void;
}) {
  return (
    <div
      className={cn(
        'absolute top-full z-50 mt-1 min-w-[220px] rounded-xl bg-white p-1 shadow-pop [-webkit-app-region:no-drag]',
        align === 'left' ? 'left-0' : 'right-0',
      )}
      role="menu"
    >
      {items.map((item, i) =>
        item === 'sep' ? (
          <div key={i} className="my-1 h-px bg-line-soft" />
        ) : (
          <button
            key={i}
            role="menuitem"
            className="flex w-full items-center justify-between gap-8 rounded-md px-3 py-[5px] text-left text-sm text-ink-soft transition-colors hover:bg-panel-hover"
            onClick={() => {
              onClose();
              item.onSelect();
            }}
          >
            <span>{item.label}</span>
            {item.shortcut && <span className="text-xs text-muted-faint">{item.shortcut}</span>}
          </button>
        ),
      )}
    </div>
  );
}

export default function TitleBar() {
  const route = useAppStore((s) => s.route);
  const sidebarVisible = useAppStore((s) => s.sidebarVisible);
  const toggleSidebar = useAppStore((s) => s.toggleSidebar);
  const toggleRightPanel = useAppStore((s) => s.toggleRightPanel);
  const rightPanelOpen = useAppStore((s) => s.rightPanelOpen);
  const openAgent = useAppStore((s) => s.openAgent);
  const openAutomations = useAppStore((s) => s.openAutomations);
  const openCustomize = useAppStore((s) => s.openCustomize);
  const setPaletteOpen = useAppStore((s) => s.setPaletteOpen);

  const [openMenu, setOpenMenu] = useState<MenuKey | null>(null);
  const [maximized, setMaximized] = useState(false);
  const rootRef = useRef<HTMLElement>(null);

  // 跟踪窗口最大化状态(浏览器环境下 bridge 为 undefined,跳过)
  useEffect(() => {
    const api = bridge();
    if (!api) return;
    const off = api.win.onMaximizedChanged(setMaximized);
    return () => {
      off();
    };
  }, []);

  // Esc / 点击外部 关闭已展开的菜单
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

  const menus: Record<Exclude<MenuKey, 'more'>, { label: string; items: MenuItem[] }> = {
    file: {
      label: 'File',
      items: [
        { label: 'New Agent', shortcut: 'Ctrl+N', onSelect: () => openAgent(null) },
        { label: 'Search…', shortcut: 'Ctrl+K', onSelect: () => setPaletteOpen(true) },
        'sep',
        { label: 'Exit', onSelect: () => bridge()?.win.close() },
      ],
    },
    edit: {
      label: 'Edit',
      items: [
        { label: 'Undo', shortcut: 'Ctrl+Z', onSelect: () => document.execCommand('undo') },
        { label: 'Redo', shortcut: 'Ctrl+Y', onSelect: () => document.execCommand('redo') },
        'sep',
        { label: 'Cut', shortcut: 'Ctrl+X', onSelect: () => document.execCommand('cut') },
        { label: 'Copy', shortcut: 'Ctrl+C', onSelect: () => document.execCommand('copy') },
        { label: 'Paste', shortcut: 'Ctrl+V', onSelect: () => document.execCommand('paste') },
      ],
    },
    view: {
      label: 'View',
      items: [
        { label: 'Reload', shortcut: 'Ctrl+R', onSelect: () => window.location.reload() },
        'sep',
        { label: 'Toggle Sidebar', onSelect: toggleSidebar },
        { label: 'Toggle Right Panel', onSelect: toggleRightPanel },
      ],
    },
    help: {
      label: 'Help',
      items: [
        { label: 'Documentation', onSelect: () => window.open('https://cursor.com/docs', '_blank') },
        { label: 'Release Notes', onSelect: () => window.open('https://cursor.com/changelog', '_blank') },
      ],
    },
  };

  const moreItems: MenuItem[] = [
    { label: 'New Agent', shortcut: 'Ctrl+N', onSelect: () => openAgent(null) },
    'sep',
    { label: 'Automations', onSelect: openAutomations },
    { label: 'Customize', onSelect: openCustomize },
    'sep',
    { label: 'Toggle Sidebar', onSelect: toggleSidebar },
    { label: 'Toggle Right Panel', onSelect: toggleRightPanel },
  ];

  const menuTriggerCls = (key: MenuKey) =>
    cn(
      'rounded-md px-2.5 py-1 text-sm transition-colors [-webkit-app-region:no-drag]',
      openMenu === key ? 'bg-panel-hover text-ink' : 'text-ink-soft hover:bg-panel-hover',
    );

  const iconBtnCls =
    'flex h-7 w-7 items-center justify-center rounded-md text-muted transition-colors hover:bg-panel-hover hover:text-ink-soft';

  return (
    <header ref={rootRef} className="shrink-0 bg-white">
      {/* 第一栏:菜单栏(可拖拽移动窗口) */}
      <div className="flex h-9 items-center [-webkit-app-region:drag]">
        <div className="flex items-center pl-3">
          <Box size={16} strokeWidth={2.25} className="text-ink" aria-label="Rurix Forge" />
        </div>
        <nav className="ml-2 flex items-center gap-0.5">
          {(Object.keys(menus) as Array<keyof typeof menus>).map((key) => (
            <div key={key} className="relative">
              <button
                className={menuTriggerCls(key)}
                onClick={() => setOpenMenu(openMenu === key ? null : key)}
                onMouseEnter={() => {
                  // 已有菜单展开时,横向划过直接切换
                  if (openMenu && openMenu !== key) setOpenMenu(key);
                }}
              >
                {menus[key].label}
              </button>
              {openMenu === key && <Dropdown items={menus[key].items} onClose={() => setOpenMenu(null)} />}
            </div>
          ))}
        </nav>

        <div className="flex-1" />

        {/* Windows 窗口控制钮 */}
        <div className="flex h-full items-stretch [-webkit-app-region:no-drag]">
          <button
            title="Minimize"
            className="flex w-11 items-center justify-center text-ink-soft transition-colors hover:bg-panel-hover"
            onClick={() => bridge()?.win.minimize()}
          >
            <Minus size={15} strokeWidth={1.5} />
          </button>
          <button
            title={maximized ? 'Restore' : 'Maximize'}
            className="flex w-11 items-center justify-center text-ink-soft transition-colors hover:bg-panel-hover"
            onClick={() => bridge()?.win.toggleMaximize()}
          >
            {maximized ? (
              <SquareSquare size={13} strokeWidth={1.5} />
            ) : (
              <Square size={13} strokeWidth={1.5} />
            )}
          </button>
          <button
            title="Close"
            className="flex w-11 items-center justify-center text-ink-soft transition-colors hover:bg-[#e81123] hover:text-white"
            onClick={() => bridge()?.win.close()}
          >
            <X size={15} strokeWidth={1.5} />
          </button>
        </div>
      </div>

      {/* 第二栏:侧栏钮 + 前进后退(与侧栏同宽同底色,分割线贯通)| 会话标题 + 右侧操作 */}
      <div className="flex h-9 items-stretch border-b border-line-soft">
        <div
          className={cn(
            'flex shrink-0 items-center justify-between border-r border-line-soft bg-panel px-2',
            sidebarVisible && 'w-[230px]',
          )}
        >
          <button title="Toggle Sidebar" className={iconBtnCls} onClick={toggleSidebar}>
            <PanelLeft size={16} strokeWidth={1.75} />
          </button>
          <div className="flex items-center gap-1">
            <button title="Back" className={iconBtnCls}>
              <ChevronLeft size={16} strokeWidth={1.75} />
            </button>
            <button title="Forward" className={iconBtnCls}>
              <ChevronRight size={16} strokeWidth={1.75} />
            </button>
          </div>
        </div>

        <div className="flex min-w-0 flex-1 items-center justify-between bg-white px-2">
          <div className="flex min-w-0 items-center gap-1.5">
            {route === 'agent' && (
              <>
                <span className="truncate text-sm font-medium text-ink">{AGENT_TITLE}</span>
                <button
                  type="button"
                  title="Open in new window"
                  className="grid h-6 w-6 shrink-0 place-items-center rounded-md text-muted-faint transition-colors hover:bg-panel-hover hover:text-muted"
                >
                  <Copy size={13} />
                </button>
              </>
            )}
          </div>
          <div className="flex items-center gap-1">
            <button
              title="Open in IDE"
              className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted transition-colors hover:bg-panel-hover hover:text-ink-soft"
            >
              IDE
              <ArrowUpRight size={12} strokeWidth={2} />
            </button>
            <div className="relative">
              <button
                title="More"
                className={cn(iconBtnCls, openMenu === 'more' && 'bg-panel-hover text-ink-soft')}
                onClick={() => setOpenMenu(openMenu === 'more' ? null : 'more')}
              >
                <MoreHorizontal size={16} strokeWidth={1.75} />
              </button>
              {openMenu === 'more' && <Dropdown items={moreItems} align="right" onClose={() => setOpenMenu(null)} />}
            </div>
            <button
              title="Toggle Right Panel"
              className={cn(iconBtnCls, rightPanelOpen && 'bg-panel-active text-ink hover:bg-panel-active hover:text-ink')}
              onClick={toggleRightPanel}
            >
              <PanelRight size={16} strokeWidth={1.75} />
            </button>
          </div>
        </div>
      </div>
    </header>
  );
}
