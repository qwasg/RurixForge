import { useEffect, useMemo, useState } from 'react';
import {
  BookOpen,
  Boxes,
  ChevronDown,
  ChevronRight,
  Folder,
  FolderPlus,
  History,
  MessagesSquare,
  Pin,
  PinOff,
  Search,
  Settings,
  Sparkles,
  Store,
  Trash2,
} from 'lucide-react';
import { cn } from '@/lib/cn';
import { useOverlayStore } from '@/lib/overlayStore';
import { isUnread, useSessionSeen } from '@/lib/sessionSeen';
import { useSessionStore, type ForgeSession } from '@/lib/sessionStore';
import { KEYS } from '@/lib/shortcuts';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { displayRoot, useWorkspaceStore } from '@/lib/workspaceStore';
import AccountCard from './AccountCard';
import { IBtn, Kbd, PaneToggleBtn, SecHead } from './primitives';
import WorkspacePicker from './WorkspacePicker';

/**
 * 会话侧栏(真实数据 = sessionStore;D-040 参考 Codex 线程栏重排):
 * 头 = 搜索框(按 / 聚焦,Shell 全局监听)+ 折叠钮同一行;New Agent / 资产商店 / Skill 管理三行导航;
 * 身 = PINNED + 文件夹组 + 会话组:选「全部会话」时未归档会话按工作区分组(Codex 的项目 → 线程),
 * 选定工作区时单列「最近」;组内 12 条上限 + More(N)。文件夹组头双击重命名。
 * 会话行:空闲不挂点,运行中 = 动态状态点,后台跑出新结果 = 未读点(lib/sessionSeen);
 * 标题 + 相对时间 + hover pin/移入文件夹/trash + 双击内联重命名。
 * 脚 = 工作区选择器(WorkspacePicker)+ 账户卡(AccountCard,真实用户名 / Codex 套餐)+ 齿轮。
 */

const GROUP_VISIBLE_LIMIT = 12;

/** 参考 relative_time:now/Nm/Nh/Nd/Nw */
export function relativeTime(iso: string | undefined): string {
  if (!iso) return '';
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return '';
  const secs = Math.max(0, Math.floor((Date.now() - then) / 1000));
  if (secs < 60) return 'now';
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h`;
  if (secs < 86400 * 7) return `${Math.floor(secs / 86400)}d`;
  return `${Math.floor(secs / (86400 * 7))}w`;
}

/** 行首状态位(固定 8px 宽,空闲留空以对齐标题):运行中涟漪点 / 未读实心点。 */
function RowIndicator({ running, unread }: { running: boolean; unread: boolean }) {
  if (running) {
    return (
      <span data-testid="session-running" title="运行中" className="relative flex h-2 w-2 shrink-0">
        <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-acc opacity-60 motion-reduce:animate-none" />
        <span className="relative inline-flex h-2 w-2 rounded-full bg-acc" />
      </span>
    );
  }
  if (unread) {
    return <span data-testid="session-unread" title="有新结果" className="h-[7px] w-[7px] shrink-0 rounded-full bg-acc" />;
  }
  return <span className="w-2 shrink-0" />;
}

function SessionRow({ s, indented, unread }: { s: ForgeSession; indented?: boolean; unread: boolean }) {
  const activeSessionId = useSessionStore((st) => st.activeSessionId);
  const select = useSessionStore((st) => st.select);
  const rename = useSessionStore((st) => st.rename);
  const togglePin = useSessionStore((st) => st.togglePin);
  const remove = useSessionStore((st) => st.remove);
  const moveToFolder = useSessionStore((st) => st.moveToFolder);
  const folders = useSessionStore((st) => st.folders);

  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(s.title);
  const [moveOpen, setMoveOpen] = useState(false);

  const isSel = activeSessionId === s.id;
  const running = s.activeRunId != null;
  const title = s.title === '' ? s.id : s.title;

  const commitRename = () => {
    setRenaming(false);
    const t = draft.trim();
    if (t !== '' && t !== s.title) void rename(s.id, t);
    else setDraft(s.title);
  };

  const actionBtn =
    'flex h-5 w-5 items-center justify-center rounded text-fg-3 transition-colors hover:bg-shell-active';

  return (
    <div
      role="button"
      tabIndex={0}
      title={title}
      data-testid={`session-row-${s.id}`}
      data-unread={unread ? '1' : undefined}
      onClick={() => select(s.id)}
      onDoubleClick={() => {
        setDraft(s.title);
        setRenaming(true);
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') select(s.id);
      }}
      className={cn(
        'group/sess relative mx-1 flex min-h-[30px] items-center gap-2 rounded-md py-1 pl-2.5 pr-1.5',
        indented && 'pl-6',
        isSel ? 'bg-shell-active' : 'hover:bg-shell-hover',
      )}
    >
      <RowIndicator running={running} unread={unread} />
      {renaming ? (
        <input
          autoFocus
          value={draft}
          data-testid={`session-rename-${s.id}`}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commitRename}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commitRename();
            if (e.key === 'Escape') {
              setDraft(s.title);
              setRenaming(false);
            }
          }}
          onClick={(e) => e.stopPropagation()}
          className="min-w-0 flex-1 rounded border border-acc-ring bg-shell-panel px-1 py-px text-[12.4px] text-fg outline-none"
        />
      ) : (
        <>
          <span
            className={cn(
              'min-w-0 flex-1 truncate text-[12.4px]',
              isSel || unread ? 'text-fg' : 'text-fg-2',
              unread && 'font-semibold',
            )}
          >
            {title}
          </span>
          <span className="shrink-0 font-code text-[10px] text-fg-4 group-hover/sess:hidden">
            {relativeTime(s.updatedAt)}
          </span>
        </>
      )}
      {/* hover 三钮:pin / 移入文件夹 / trash(与相对时间同位互换,行宽不跳) */}
      {!renaming && (
        <span className="hidden shrink-0 items-center gap-0.5 group-hover/sess:flex">
          <button
            type="button"
            title={s.pinned ? '取消置顶' : '置顶'}
            aria-label={s.pinned ? '取消置顶' : '置顶'}
            className={actionBtn}
            onClick={(e) => {
              e.stopPropagation();
              void togglePin(s.id);
            }}
          >
            {s.pinned ? <PinOff size={11} /> : <Pin size={11} />}
          </button>
          <button
            type="button"
            title="移入文件夹"
            aria-label="移入文件夹"
            className={actionBtn}
            onClick={(e) => {
              e.stopPropagation();
              setMoveOpen((v) => !v);
            }}
          >
            <Folder size={11} />
          </button>
          <button
            type="button"
            title="删除会话"
            aria-label="删除会话"
            className={cn(actionBtn, 'hover:bg-danger-bg')}
            onClick={(e) => {
              e.stopPropagation();
              void remove(s.id);
            }}
          >
            <Trash2 size={11} />
          </button>
        </span>
      )}
      {/* 移入文件夹弹层 */}
      {moveOpen && (
        <div className="absolute right-2 top-7 z-40 min-w-[140px] max-w-[200px] rounded-lg border border-edge bg-shell-panel p-1 shadow-sh1">
          <button
            type="button"
            className="flex w-full items-center rounded-[5px] px-2 py-1 text-left text-[11px] text-fg-2 hover:bg-shell-hover"
            onClick={(e) => {
              e.stopPropagation();
              setMoveOpen(false);
              void moveToFolder(s.id, null);
            }}
          >
            移出文件夹
          </button>
          {folders.map((f) => (
            <button
              key={f.id}
              type="button"
              className="flex w-full items-center rounded-[5px] px-2 py-1 text-left text-[11px] text-fg hover:bg-shell-hover"
              onClick={(e) => {
                e.stopPropagation();
                setMoveOpen(false);
                void moveToFolder(s.id, f.id);
              }}
            >
              <span className="truncate">{f.name}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

type GroupKind = 'folder' | 'workspace' | 'recent';

interface Group {
  key: string;
  label: string;
  kind: GroupKind;
  folderId: string | null;
  sessions: ForgeSession[];
  /** 组头悬停说明(工作区组 = 根路径)。 */
  title?: string;
}

const GROUP_ICON: Record<GroupKind, typeof Folder> = {
  folder: Folder,
  workspace: Boxes,
  recent: History,
};

function latest(list: ForgeSession[]): number {
  return list.reduce((m, s) => Math.max(m, Date.parse(s.updatedAt) || 0), 0);
}

function GroupHeader({
  g,
  collapsed,
  onToggle,
}: {
  g: Group;
  collapsed: boolean;
  onToggle: () => void;
}) {
  const removeFolder = useSessionStore((st) => st.removeFolder);
  const renameFolder = useSessionStore((st) => st.renameFolder);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(g.label);
  const Icon = GROUP_ICON[g.kind];

  const commit = () => {
    setRenaming(false);
    if (g.folderId && draft.trim() !== '' && draft.trim() !== g.label) void renameFolder(g.folderId, draft);
  };

  return (
    <div
      role="button"
      tabIndex={0}
      title={g.folderId ? `${g.label}(双击重命名)` : g.title}
      data-testid={`session-group-${g.key}`}
      onClick={() => {
        if (!renaming) onToggle();
      }}
      onDoubleClick={() => {
        if (!g.folderId) return;
        setDraft(g.label);
        setRenaming(true);
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && !renaming) onToggle();
      }}
      className="group/ws mx-1 flex h-7 items-center gap-1.5 rounded-md px-2 text-[12px] text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2"
    >
      {collapsed ? (
        <ChevronRight size={11} className="shrink-0 text-fg-4" />
      ) : (
        <ChevronDown size={11} className="shrink-0 text-fg-4" />
      )}
      <Icon size={12} className="shrink-0" />
      {renaming ? (
        <input
          autoFocus
          value={draft}
          data-testid={`folder-rename-${g.folderId}`}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter') commit();
            if (e.key === 'Escape') setRenaming(false);
          }}
          className="min-w-0 flex-1 rounded border border-acc-ring bg-shell-panel px-1 py-px text-[12px] text-fg outline-none"
        />
      ) : (
        <span className="min-w-0 flex-1 truncate font-medium">{g.label}</span>
      )}
      {!renaming && (
        <span className="shrink-0 font-code text-[10px] text-fg-4 group-hover/ws:hidden">{g.sessions.length}</span>
      )}
      {g.folderId && !renaming && (
        <button
          type="button"
          title="删除文件夹"
          aria-label="删除文件夹"
          onClick={(e) => {
            e.stopPropagation();
            void removeFolder(g.folderId as string);
          }}
          className="hidden h-5 w-5 items-center justify-center rounded text-fg-3 hover:bg-danger-bg group-hover/ws:flex"
        >
          <Trash2 size={10} />
        </button>
      )}
    </div>
  );
}

export default function Sidebar() {
  const sessions = useSessionStore((st) => st.sessions);
  const folders = useSessionStore((st) => st.folders);
  const offline = useSessionStore((st) => st.offline);
  const activeSessionId = useSessionStore((st) => st.activeSessionId);
  const create = useSessionStore((st) => st.create);
  const createFolder = useSessionStore((st) => st.createFolder);
  const workspaces = useWorkspaceStore((st) => st.workspaces);
  const activeWorkspaceId = useWorkspaceStore((st) => st.activeWorkspaceId);
  const openSettings = useOverlayStore((st) => st.open);
  const openTab = useWorkbenchStore((st) => st.openTab);
  const activeTabId = useWorkbenchStore((st) => st.activeTabId);
  const seen = useSessionSeen((st) => st.seen);
  const observe = useSessionSeen((st) => st.observe);
  const markSeen = useSessionSeen((st) => st.markSeen);

  const [query, setQuery] = useState('');
  const [creatingFolder, setCreatingFolder] = useState(false);
  const [folderName, setFolderName] = useState('');
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [showAll, setShowAll] = useState<ReadonlySet<string>>(new Set());

  // 已读基线:新出现的会话以当时的 updatedAt 为基线;正在看的会话随更新持续标已读
  useEffect(() => {
    observe(sessions);
  }, [observe, sessions]);
  const activeSession = sessions.find((s) => s.id === activeSessionId);
  useEffect(() => {
    if (activeSession) markSeen(activeSession);
    // updatedAt 前进(当前会话出新结果)也算已读
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeSession?.id, activeSession?.updatedAt, markSeen]);

  const workspaceFilter = (workspaceId?: string | null) =>
    activeWorkspaceId === null || workspaceId === activeWorkspaceId;

  const visibleFolders = useMemo(
    () => folders.filter((f) => workspaceFilter(f.workspaceId)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [folders, activeWorkspaceId],
  );

  const q = query.trim().toLowerCase();
  const matches = (s: ForgeSession) =>
    q === '' || s.title.toLowerCase().includes(q) || s.id.toLowerCase().includes(q);

  const groups = useMemo<Group[]>(() => {
    const matched = sessions.filter((s) => workspaceFilter(s.workspaceId) && !s.pinned && matches(s));
    const folderIds = new Set(visibleFolders.map((f) => f.id));
    const byFolder = new Map<string, ForgeSession[]>();
    const unfiled: ForgeSession[] = [];
    for (const s of matched) {
      if (s.folderId && folderIds.has(s.folderId)) {
        const arr = byFolder.get(s.folderId) ?? [];
        arr.push(s);
        byFolder.set(s.folderId, arr);
      } else {
        unfiled.push(s);
      }
    }
    const out: Group[] = visibleFolders.map((f) => ({
      key: `folder:${f.id}`,
      label: f.name,
      kind: 'folder',
      folderId: f.id,
      sessions: byFolder.get(f.id) ?? [],
    }));
    if (activeWorkspaceId !== null) {
      out.push({ key: 'plain', label: '最近', kind: 'recent', folderId: null, sessions: unfiled });
      return out;
    }
    // 全部会话:按工作区分组,组按最近活动排序;未绑定(或绑定的工作区已删)的会话落在默认根执行,归「默认工作区」
    const known = new Map(workspaces.map((w) => [w.id, w] as const));
    const byWs = new Map<string, ForgeSession[]>();
    const loose: ForgeSession[] = [];
    for (const s of unfiled) {
      if (s.workspaceId && known.has(s.workspaceId)) {
        const arr = byWs.get(s.workspaceId) ?? [];
        arr.push(s);
        byWs.set(s.workspaceId, arr);
      } else {
        loose.push(s);
      }
    }
    const wsGroups: Group[] = [...byWs.entries()].map(([id, list]) => {
      const w = known.get(id);
      return {
        key: `ws:${id}`,
        label: w?.name ?? id,
        kind: 'workspace',
        folderId: null,
        sessions: list,
        title: w ? displayRoot(w.root) : undefined,
      };
    });
    if (loose.length > 0) {
      wsGroups.push({ key: 'ws:none', label: '默认工作区', kind: 'workspace', folderId: null, sessions: loose });
    }
    wsGroups.sort((a, b) => latest(b.sessions) - latest(a.sessions));
    return [...out, ...wsGroups];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessions, visibleFolders, workspaces, q, activeWorkspaceId]);

  const pinned = useMemo(
    () => sessions.filter((s) => workspaceFilter(s.workspaceId) && s.pinned && matches(s)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [sessions, q, activeWorkspaceId],
  );

  const toggleIn = (setter: typeof setCollapsed, key: string) =>
    setter((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const submitFolder = () => {
    const name = folderName.trim();
    if (name === '') return;
    setFolderName('');
    setCreatingFolder(false);
    void createFolder(name);
  };

  const empty = pinned.length === 0 && groups.every((g) => g.sessions.length === 0);
  const unreadOf = (s: ForgeSession) => isUnread(s, seen[s.id], activeSessionId);

  const navRow = (active: boolean) =>
    cn(
      'flex h-7 items-center gap-2 rounded-md px-2 text-left text-[12.4px] text-fg-2 transition-colors hover:bg-shell-hover hover:text-fg',
      active && 'bg-shell-active text-fg',
    );

  return (
    <div data-testid="sidebar" className="flex h-full min-h-0 flex-col bg-shell-sidebar">
      {/* 头:搜索(/ 聚焦)+ 折叠钮同一行;下挂三行导航 */}
      <div className="flex shrink-0 flex-col gap-1 p-2.5 pb-2">
        <div className="mb-1 flex items-center gap-1">
          <div className="flex h-[30px] min-w-0 flex-1 items-center gap-1.5 rounded-md border border-edge bg-shell-panel px-2 focus-within:border-acc-ring">
            <Search size={12} className="shrink-0 text-fg-3" />
            <input
              value={query}
              data-testid="sidebar-search"
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  setQuery('');
                  e.currentTarget.blur();
                }
              }}
              placeholder="搜索会话…"
              className="h-full min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-4"
            />
            <Kbd label={KEYS.focusSessionSearch} />
          </div>
          <PaneToggleBtn kind="sessions" className="h-[30px] w-7" />
        </div>
        <button type="button" data-testid="sidebar-new-agent" onClick={() => void create()} className={navRow(false)}>
          <Sparkles size={13} className="shrink-0 text-acc" />
          <span className="min-w-0 flex-1">New Agent</span>
          <Kbd label={KEYS.newSession} />
        </button>
        {/*
          F11(D-025):两个大类入口。不新增常驻面板(I-3 七区冻结),点击开 workbench tab
          ——与 plan/todo/proposals 同承载路径。activeTabId 命中时高亮,重复点击不重复开。
        */}
        <button
          type="button"
          data-testid="sidebar-asset-store"
          onClick={() => openTab('store')}
          className={navRow(activeTabId === 'store')}
        >
          <Store size={13} className="shrink-0 text-fg-3" />
          <span className="min-w-0 flex-1">资产商店</span>
        </button>
        <button
          type="button"
          data-testid="sidebar-skills"
          onClick={() => openTab('skills')}
          className={navRow(activeTabId === 'skills')}
        >
          <BookOpen size={13} className="shrink-0 text-fg-3" />
          <span className="min-w-0 flex-1">Skill 管理</span>
        </button>
      </div>

      {/* 身 */}
      <div className="min-h-0 flex-1 overflow-y-auto pb-1">
        {offline && (
          <div className="mx-3 mb-2.5 rounded-lg border border-edge bg-shell-panel p-2 text-[11px] text-fg-3">
            后端未连接
          </div>
        )}

        {empty && !offline && (
          <p className="p-3.5 text-[12px] text-fg-4">
            {q !== '' ? '没有匹配的会话。' : '暂无会话，可点击「New Agent」创建。'}
          </p>
        )}

        {pinned.length > 0 && (
          <>
            <SecHead icon={<Pin size={10} />} label="PINNED" />
            {pinned.map((s) => (
              <SessionRow key={s.id} s={s} unread={unreadOf(s)} />
            ))}
          </>
        )}

        <SecHead icon={<MessagesSquare size={10} />} label="会话">
          <span className="ml-auto flex items-center gap-0.5">
            <button
              type="button"
              title="新建文件夹"
              aria-label="新建文件夹"
              data-testid="sidebar-new-folder"
              onClick={() => {
                setCreatingFolder((v) => !v);
                setFolderName('');
              }}
              className="flex h-[22px] w-[22px] items-center justify-center rounded-[5px] text-fg-3 transition-colors hover:bg-shell-hover"
            >
              <FolderPlus size={11} />
            </button>
          </span>
        </SecHead>

        {creatingFolder && (
          <div className="mx-2.5 mb-1 flex h-7 items-center gap-1.5 rounded-md border border-acc-ring bg-shell-panel px-2">
            <FolderPlus size={12} className="shrink-0 text-fg-3" />
            <input
              autoFocus
              value={folderName}
              data-testid="sidebar-folder-name"
              onChange={(e) => setFolderName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') submitFolder();
                if (e.key === 'Escape') {
                  setCreatingFolder(false);
                  setFolderName('');
                }
              }}
              placeholder="文件夹名…"
              className="min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-4"
            />
            <button type="button" onClick={submitFolder} className="shrink-0 text-[11px] text-acc">
              创建
            </button>
          </div>
        )}

        {groups.map((g) => {
          const isCollapsed = collapsed.has(g.key);
          const all = showAll.has(g.key);
          const overflow = g.sessions.length - GROUP_VISIBLE_LIMIT;
          const visible = all || overflow <= 0 ? g.sessions : g.sessions.slice(0, GROUP_VISIBLE_LIMIT);
          return (
            <div key={g.key} className="flex flex-col">
              <GroupHeader g={g} collapsed={isCollapsed} onToggle={() => toggleIn(setCollapsed, g.key)} />
              {!isCollapsed && (
                <>
                  {visible.map((s) => (
                    <SessionRow key={s.id} s={s} indented unread={unreadOf(s)} />
                  ))}
                  {overflow > 0 && (
                    <button
                      type="button"
                      onClick={() => toggleIn(setShowAll, g.key)}
                      className="mx-2.5 my-0.5 ml-[22px] flex items-center gap-[5px] rounded-md px-2 py-[3px] text-left text-[11px] text-fg-3 transition-colors hover:bg-shell-hover"
                    >
                      {all ? <ChevronDown size={11} /> : <ChevronDown size={11} className="rotate-180" />}
                      {all ? '收起' : `More (${overflow})`}
                    </button>
                  )}
                </>
              )}
            </div>
          );
        })}
      </div>

      {/* 工作区选择器:贴在账户卡上方,展开时把自己顶上去、面板落在空隙里 */}
      <WorkspacePicker />

      {/* 脚:账户卡(真实用户名 / Codex 套餐,点开账户菜单)+ 设置齿轮 */}
      <div className="flex shrink-0 items-center gap-1 border-t border-edge p-2">
        <AccountCard />
        <IBtn title="设置" testId="sidebar-settings" onClick={() => openSettings('settings')}>
          <Settings size={13} />
        </IBtn>
      </div>
    </div>
  );
}
