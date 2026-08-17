import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import {
  Archive,
  Bot,
  ChevronDown,
  ChevronRight,
  Cloud,
  Filter,
  Folder,
  FolderOpen,
  FolderPlus,
  GitBranch,
  Home,
  ListFilter,
  Monitor,
  Pin,
  PinOff,
  Plus,
  Search,
  Settings,
  SlidersHorizontal,
} from 'lucide-react';
import { useAppStore } from '@/lib/store';
import { WORKSPACE_RECENTS } from '@/lib/mock';
import type { SidebarAgent, Workspace } from '@/lib/types';
import { cn } from '@/lib/cn';

/** 顶部导航行(New Agent / Search / Automations / Customize)。 */
function NavRow({
  icon,
  label,
  active,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn('nav-row px-1', active && 'bg-panel-active')}
    >
      <span className="flex h-[18px] w-[18px] shrink-0 items-center justify-center text-ink-soft">
        {icon}
      </span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
    </button>
  );
}

/** 行内 hover 操作钮(pin / archive)。 */
const actionBtn =
  'grid h-[18px] w-[18px] place-items-center rounded text-muted-faint transition-colors hover:bg-panel-active hover:text-muted';

/**
 * Pinned / workspace 下的会话行。
 * 常态:标题单行省略 + 右侧 ago;active 带蓝点;cloud/branch 带对应图标。
 * 悬停:标题右侧浮出 pin( pinned 行为 unpin)+ archive 钮;pinned 行额外弹出详情卡。
 */
function AgentRow({ agent, pinned }: { agent: SidebarAgent; pinned?: boolean }) {
  const activeAgentId = useAppStore((s) => s.activeAgentId);
  const openAgent = useAppStore((s) => s.openAgent);
  const pinAgent = useAppStore((s) => s.pinAgent);
  const unpinAgent = useAppStore((s) => s.unpinAgent);
  const archiveAgent = useAppStore((s) => s.archiveAgent);

  const rowRef = useRef<HTMLDivElement>(null);
  const [tip, setTip] = useState<{ x: number; y: number } | null>(null);

  // pinned 行悬停时在侧栏右侧弹出详情卡(fixed 定位,避免被滚动容器裁切)
  const showTip = () => {
    if (!pinned || !agent.repo) return;
    const r = rowRef.current?.getBoundingClientRect();
    if (r) setTip({ x: r.right + 8, y: r.top - 6 });
  };

  return (
    <div ref={rowRef} onMouseEnter={showTip} onMouseLeave={() => setTip(null)}>
      <div
        role="button"
        tabIndex={0}
        title={agent.title}
        onClick={() => openAgent(agent.id)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') openAgent(agent.id);
        }}
        className={cn(
          'nav-row group/row cursor-pointer px-1',
          activeAgentId === agent.id && 'bg-panel-active',
        )}
      >
        <span className="flex min-w-0 flex-1 items-center">
          <span className="flex w-[26px] shrink-0 items-center justify-center">
            {agent.active && <span className="h-1.5 w-1.5 rounded-full bg-accent-blue" />}
          </span>
          <span className="truncate">{agent.title}</span>
        </span>
        <span className="ml-1 flex shrink-0 items-center gap-0.5">
          <span className="hidden items-center gap-0.5 group-hover/row:flex">
            <button
              type="button"
              title={pinned ? 'Unpin' : 'Pin'}
              className={actionBtn}
              onClick={(e) => {
                e.stopPropagation();
                if (pinned) unpinAgent(agent.id);
                else pinAgent(agent.id);
              }}
            >
              {pinned ? <PinOff size={12} strokeWidth={1.8} /> : <Pin size={12} strokeWidth={1.8} />}
            </button>
            <button
              type="button"
              title="Archive"
              className={actionBtn}
              onClick={(e) => {
                e.stopPropagation();
                archiveAgent(agent.id);
              }}
            >
              <Archive size={12} strokeWidth={1.8} />
            </button>
          </span>
          <span className="flex items-center gap-1 text-xs text-muted-faint">
            {agent.cloud && <Cloud size={12} strokeWidth={1.8} />}
            {agent.branch && <GitBranch size={12} strokeWidth={1.8} />}
            {agent.ago}
          </span>
        </span>
      </div>

      {tip && agent.repo && (
        <div
          className="fixed z-50 w-56 rounded-xl bg-white p-3 shadow-pop"
          style={{ left: tip.x, top: tip.y }}
        >
          <div className="truncate text-sm font-medium text-ink">{agent.title}</div>
          <div className="mt-2 flex items-center gap-2 text-xs text-muted">
            <GitBranch size={12} className="shrink-0" />
            <span className="truncate">{agent.repo}</span>
          </div>
          <div className="mt-0.5 pl-5 text-xs text-muted-faint">{agent.branchName}</div>
          <div className="mt-2 flex items-center gap-2 text-xs text-muted">
            <FolderOpen size={12} className="shrink-0" />
            <span className="truncate">{agent.path}</span>
          </div>
        </div>
      )}
    </div>
  );
}

/** 一个 workspace 分组:文件夹行(悬停变折叠箭头 + 右侧 "+")+ 会话列表 + 可选 More 行。 */
function WorkspaceSection({ workspace }: { workspace: Workspace }) {
  const goHome = useAppStore((s) => s.goHome);
  const [collapsed, setCollapsed] = useState(false);

  return (
    <div className="mt-0.5">
      <div
        role="button"
        tabIndex={0}
        onClick={() => setCollapsed((c) => !c)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') setCollapsed((c) => !c);
        }}
        className="nav-row group/ws cursor-pointer px-1"
      >
        <span className="flex h-[18px] w-[18px] shrink-0 items-center justify-center text-muted">
          <FolderOpen size={14} strokeWidth={1.8} className="group-hover/ws:hidden" />
          <ChevronDown
            size={14}
            strokeWidth={1.8}
            className={cn('hidden group-hover/ws:block', collapsed && '-rotate-90')}
          />
        </span>
        <span className="min-w-0 flex-1 truncate text-left">{workspace.name}</span>
        <button
          type="button"
          title="New agent in workspace"
          className="hidden h-[18px] w-[18px] place-items-center rounded text-muted-faint transition-colors hover:bg-panel-active hover:text-muted group-hover/ws:grid"
          onClick={(e) => {
            e.stopPropagation();
            goHome();
          }}
        >
          <Plus size={13} strokeWidth={1.8} />
        </button>
      </div>
      {!collapsed && workspace.agents.map((agent) => <AgentRow key={agent.id} agent={agent} />)}
      {!collapsed && workspace.hasMore && (
        <button type="button" className="nav-row px-1 text-muted">
          <span className="w-[26px] shrink-0" />
          More
        </button>
      )}
    </div>
  );
}

/** Workspaces "+" 弹层:搜索框 + Recents 文件夹 + Repos 来源。 */
function WorkspaceMenu({ x, y, onClose }: { x: number; y: number; onClose: () => void }) {
  const [q, setQ] = useState('');
  const recents = WORKSPACE_RECENTS.filter((p) =>
    p.toLowerCase().includes(q.trim().toLowerCase()),
  );
  const itemRow =
    'flex w-full items-center gap-2 rounded-md px-2 py-[5px] text-left text-sm text-ink-soft transition-colors hover:bg-panel-hover';
  const itemIcon = 'shrink-0 text-muted';

  return (
    <div
      data-ws-menu
      className="fixed z-50 w-[240px] rounded-xl bg-white p-1 shadow-pop"
      style={{ left: x, top: y }}
    >
      <div className="p-1">
        <input
          autoFocus
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="Search folders, repos..."
          className="w-full rounded-md border border-line bg-white px-2.5 py-1.5 text-sm text-ink outline-none placeholder:text-muted-faint focus:border-muted-faint"
        />
      </div>

      {recents.length > 0 && (
        <>
          <div className="px-2 pb-0.5 pt-1.5 text-2xs text-muted-faint">Recents</div>
          {recents.map((p) => (
            <button key={p} type="button" className={itemRow} onClick={onClose}>
              <Folder size={14} strokeWidth={1.8} className={itemIcon} />
              <span className="truncate">{p}</span>
            </button>
          ))}
        </>
      )}

      <div className="px-2 pb-0.5 pt-2 text-2xs text-muted-faint">Repos</div>
      <button type="button" className={itemRow} onClick={onClose}>
        <Home size={14} strokeWidth={1.8} className={itemIcon} />
        <span className="flex-1 truncate">No Repo</span>
      </button>
      <button type="button" className={itemRow} onClick={onClose}>
        <Monitor size={14} strokeWidth={1.8} className={itemIcon} />
        <span className="flex-1 truncate">On This PC</span>
        <ChevronRight size={13} className="shrink-0 text-muted-faint" />
      </button>
      <button type="button" className={itemRow} onClick={onClose}>
        <Cloud size={14} strokeWidth={1.8} className={itemIcon} />
        <span className="flex-1 truncate">Cloud</span>
        <ChevronRight size={13} className="shrink-0 text-muted-faint" />
      </button>
      <button type="button" className={cn(itemRow, 'mt-1')} onClick={onClose}>
        <FolderOpen size={14} strokeWidth={1.8} className={itemIcon} />
        <span className="flex-1 truncate">Use Existing…</span>
        <ChevronRight size={13} className="shrink-0 text-muted-faint" />
      </button>
      <button type="button" className={itemRow} onClick={onClose}>
        <FolderPlus size={14} strokeWidth={1.8} className={itemIcon} />
        <span className="flex-1 truncate">New Folder</span>
      </button>
    </div>
  );
}

export default function Sidebar() {
  const route = useAppStore((s) => s.route);
  const goHome = useAppStore((s) => s.goHome);
  const openAutomations = useAppStore((s) => s.openAutomations);
  const openCustomize = useAppStore((s) => s.openCustomize);
  const setPaletteOpen = useAppStore((s) => s.setPaletteOpen);
  const pinnedAgents = useAppStore((s) => s.pinnedAgents);
  const workspaces = useAppStore((s) => s.workspaces);
  const archiveAllPinned = useAppStore((s) => s.archiveAllPinned);

  const addBtnRef = useRef<HTMLButtonElement>(null);
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);

  const openMenu = () => {
    const r = addBtnRef.current?.getBoundingClientRect();
    if (r) setMenuAt({ x: r.left - 8, y: r.bottom + 6 });
  };

  // Esc / 点击外部 关闭 workspace 弹层
  useEffect(() => {
    if (!menuAt) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenuAt(null);
    };
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      const inMenu = (document.querySelector('[data-ws-menu]') as HTMLElement | null)?.contains(t);
      if (!inMenu && !addBtnRef.current?.contains(t)) setMenuAt(null);
    };
    window.addEventListener('keydown', onKey);
    document.addEventListener('mousedown', onDown);
    return () => {
      window.removeEventListener('keydown', onKey);
      document.removeEventListener('mousedown', onDown);
    };
  }, [menuAt]);

  const headerIconBtn =
    'flex h-[18px] w-[18px] items-center justify-center rounded text-muted-faint transition-colors hover:bg-panel-hover hover:text-muted';

  return (
    <aside className="flex w-[230px] shrink-0 flex-col border-r border-line-soft bg-panel">
      <div className="min-h-0 flex-1 overflow-y-auto px-1 pb-1 pt-1.5">
        {/* 导航组 */}
        <NavRow
          icon={<Filter size={14} strokeWidth={1.8} />}
          label="New Agent"
          active={route === 'home'}
          onClick={goHome}
        />
        <NavRow
          icon={<Search size={14} strokeWidth={1.8} />}
          label="Search"
          onClick={() => setPaletteOpen(true)}
        />
        <NavRow
          icon={<Bot size={14} strokeWidth={1.8} />}
          label="Automations"
          active={route === 'automations'}
          onClick={openAutomations}
        />
        <NavRow
          icon={<SlidersHorizontal size={14} strokeWidth={1.8} />}
          label="Customize"
          active={route === 'customize'}
          onClick={openCustomize}
        />

        {/* Pinned:整组悬停时标题右侧浮出 Archive 钮 */}
        {pinnedAgents.length > 0 && (
          <div className="group/pinned">
            <div className="flex items-center justify-between px-2 pb-0.5 pt-4">
              <span className="text-2xs text-muted-faint">Pinned</span>
              <button
                type="button"
                onClick={archiveAllPinned}
                className="rounded-full border border-line bg-white px-2 py-px text-2xs text-ink-soft opacity-0 shadow-sm transition-opacity hover:bg-panel-hover group-hover/pinned:opacity-100"
              >
                Archive
              </button>
            </div>
            {pinnedAgents.map((agent) => (
              <AgentRow key={agent.id} agent={agent} pinned />
            ))}
          </div>
        )}

        {/* Workspaces */}
        <div className="flex items-center justify-between px-2 pb-0.5 pt-4">
          <span className="text-2xs text-muted-faint">Workspaces</span>
          <span className="flex items-center gap-0.5">
            <button type="button" title="Filter workspaces" className={headerIconBtn}>
              <ListFilter size={13} strokeWidth={1.8} />
            </button>
            <button
              ref={addBtnRef}
              type="button"
              title="Add workspace"
              className={headerIconBtn}
              onClick={() => (menuAt ? setMenuAt(null) : openMenu())}
            >
              <FolderPlus size={13} strokeWidth={1.8} />
            </button>
          </span>
        </div>
        {workspaces.map((workspace) => (
          <WorkspaceSection key={workspace.id} workspace={workspace} />
        ))}
      </div>

      {menuAt && <WorkspaceMenu x={menuAt.x} y={menuAt.y} onClose={() => setMenuAt(null)} />}

      {/* 底部用户卡 */}
      <div className="flex shrink-0 items-center gap-2.5 p-2">
        <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-line text-2xs font-medium text-ink-soft">
          K
        </span>
        <span className="flex min-w-0 flex-1 flex-col leading-tight">
          <span className="truncate text-sm text-ink-soft">Kai Li</span>
          <span className="truncate text-xs text-muted">Pro Plan</span>
        </span>
        <button
          type="button"
          title="Settings"
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-muted-faint transition-colors hover:bg-panel-hover hover:text-muted"
        >
          <Settings size={15} strokeWidth={1.8} />
        </button>
      </div>
    </aside>
  );
}
