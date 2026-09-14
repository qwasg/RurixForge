import { useMemo, useState } from 'react';
import {
  BookOpen,
  ChevronDown,
  ChevronRight,
  Folder,
  FolderPlus,
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
import { useSessionStore, type ForgeSession } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { displayRoot, useWorkspaceStore } from '@/lib/workspaceStore';
import { IBtn, Kbd, PaneToggleBtn, SecHead, StatusDot } from './primitives';
import WorkspacePicker from './WorkspacePicker';

/**
 * F7 wave.3 会话侧栏(参考 ui/sidebar.rs,真实数据 = sessionStore):
 * 30px 搜索框(本地过滤标题/id);New Agent 行(POST /sessions 并选中);
 * PINNED 区;CHAT FOLDERS(内联建文件夹/组头折叠/组内 12 条上限 + More(N)/收起;
 * 无文件夹时单一「会话」组);会话行 = 6×6 状态点(activeRunId→accent 脉冲,否则 idle 灰)
 * + 标题(12.4px 截断)+ 相对时间(mono 10px)+ hover pin/移入文件夹/trash 三钮
 * + 双击内联重命名(PATCH title);底部工作区选择器(WorkspacePicker)+ 用户卡
 * (占位「本地用户」+ 齿轮开设置)。
 *
 * 2026-08-25 用户拍板:WORKSPACES 区块由会话列上方搬到用户卡上方,收起态只留一条触发条,
 * 工作区行/新建表单随之迁入 WorkspacePicker 面板(会话按 activeWorkspaceId 过滤的逻辑不变)。
 *
 * 差异留痕:参考的 workspace 分组依赖 workspaceRoot 字段(本仓会话模型未落地),
 * 本波仅 chat-folders 分组 + 单一「会话」组,workspace 分组缺失如实留档。
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

function SessionRow({ s, indented }: { s: ForgeSession; indented?: boolean }) {
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
      onClick={() => select(s.id)}
      onDoubleClick={() => {
        setDraft(s.title);
        setRenaming(true);
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') select(s.id);
      }}
      className={cn(
        'group/sess relative mx-1 flex min-h-[32px] items-center gap-1.5 rounded-md py-1 pl-3 pr-1.5',
        indented && 'pl-7',
        isSel ? 'bg-shell-active' : 'hover:bg-shell-hover',
      )}
    >
      <StatusDot color={running ? 'var(--dot-running)' : 'var(--dot-idle)'} pulse={running} />
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
          <span className="min-w-0 flex-1 truncate text-[12.4px] text-fg">{title}</span>
          <span className="shrink-0 font-code text-[10px] text-fg-4">{relativeTime(s.updatedAt)}</span>
        </>
      )}
      {/* hover 三钮:pin / 移入文件夹 / trash */}
      {!renaming && (
        <span className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover/sess:opacity-100">
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

interface Group {
  key: string;
  label: string;
  folderId: string | null;
  sessions: ForgeSession[];
}

export default function Sidebar() {
  const sessions = useSessionStore((st) => st.sessions);
  const folders = useSessionStore((st) => st.folders);
  const offline = useSessionStore((st) => st.offline);
  const create = useSessionStore((st) => st.create);
  const createFolder = useSessionStore((st) => st.createFolder);
  const removeFolder = useSessionStore((st) => st.removeFolder);
  const workspaces = useWorkspaceStore((st) => st.workspaces);
  const activeWorkspaceId = useWorkspaceStore((st) => st.activeWorkspaceId);
  const openSettings = useOverlayStore((st) => st.open);
  const openTab = useWorkbenchStore((st) => st.openTab);
  const activeTabId = useWorkbenchStore((st) => st.activeTabId);

  const [query, setQuery] = useState('');
  const [creatingFolder, setCreatingFolder] = useState(false);
  const [folderName, setFolderName] = useState('');
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [showAll, setShowAll] = useState<ReadonlySet<string>>(new Set());

  const workspaceFilter = (workspaceId?: string | null) =>
    activeWorkspaceId === null || workspaceId === activeWorkspaceId;

  const visibleFolders = useMemo(
    () => folders.filter((f) => workspaceFilter(f.workspaceId)),
    [folders, activeWorkspaceId],
  );

  const groups = useMemo<Group[]>(() => {
    const q = query.trim().toLowerCase();
    const matched = sessions.filter(
      (s) =>
        workspaceFilter(s.workspaceId) &&
        !s.pinned &&
        (q === '' || s.title.toLowerCase().includes(q) || s.id.toLowerCase().includes(q)),
    );
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
      folderId: f.id,
      sessions: byFolder.get(f.id) ?? [],
    }));
    out.push({ key: 'plain', label: '会话', folderId: null, sessions: unfiled });
    return out;
  }, [sessions, visibleFolders, query, activeWorkspaceId]);

  const pinned = useMemo(() => {
    const q = query.trim().toLowerCase();
    return sessions.filter(
      (s) =>
        workspaceFilter(s.workspaceId) &&
        s.pinned &&
        (q === '' || s.title.toLowerCase().includes(q) || s.id.toLowerCase().includes(q)),
    );
  }, [sessions, query, activeWorkspaceId]);

  const toggleCollapsed = (key: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  const toggleShowAll = (key: string) =>
    setShowAll((prev) => {
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

  const activeWorkspace = workspaces.find((w) => w.id === activeWorkspaceId);
  const empty = pinned.length === 0 && groups.every((g) => g.sessions.length === 0);

  return (
    <div data-testid="sidebar" className="flex h-full min-h-0 flex-col bg-shell-sidebar">
      {/* head:搜索 + New Agent */}
      <div className="flex shrink-0 flex-col gap-1.5 p-2.5">
        <div className="flex h-[30px] min-h-[30px] items-center gap-1.5 rounded-md border border-edge bg-shell-panel px-2 py-0.5">
          <Search size={12} className="shrink-0 text-fg-3" />
          <input
            value={query}
            data-testid="sidebar-search"
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索会话…"
            className="h-full min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-4"
          />
          <Kbd label="/" />
        </div>
        <PaneToggleBtn kind="sessions" className="h-7 w-7" />
        <button
          type="button"
          data-testid="sidebar-new-agent"
          onClick={() => void create()}
          className="flex h-7 items-center gap-2 rounded-md px-2 text-left text-[12.4px] text-fg-2 transition-colors hover:bg-shell-hover"
        >
          <Sparkles size={13} className="shrink-0 text-fg-3" />
          <span className="min-w-0 flex-1">New Agent</span>
          <Kbd label="Ctrl+Shift+N" />
        </button>
        {/*
          F11(D-025):两个大类入口。不新增常驻面板(I-3 七区冻结),点击开 workbench tab
          ——与 plan/todo/proposals 同承载路径。activeTabId 命中时高亮,重复点击不重复开。
        */}
        <button
          type="button"
          data-testid="sidebar-asset-store"
          onClick={() => openTab('store')}
          className={cn(
            'flex h-7 items-center gap-2 rounded-md px-2 text-left text-[12.4px] text-fg-2 transition-colors hover:bg-shell-hover',
            activeTabId === 'store' && 'bg-shell-active',
          )}
        >
          <Store size={13} className="shrink-0 text-fg-3" />
          <span className="min-w-0 flex-1">资产商店</span>
        </button>
        <button
          type="button"
          data-testid="sidebar-skills"
          onClick={() => openTab('skills')}
          className={cn(
            'flex h-7 items-center gap-2 rounded-md px-2 text-left text-[12.4px] text-fg-2 transition-colors hover:bg-shell-hover',
            activeTabId === 'skills' && 'bg-shell-active',
          )}
        >
          <BookOpen size={13} className="shrink-0 text-fg-3" />
          <span className="min-w-0 flex-1">Skill 管理</span>
        </button>
      </div>

      {/* body */}
      <div className="min-h-0 flex-1 overflow-y-auto pb-1">
        {offline && (
          <div className="mx-3 mb-2.5 rounded-lg border border-edge bg-shell-panel p-2 text-[11px] text-fg-3">
            后端未连接
          </div>
        )}

        {empty && !offline && (
          <p className="p-3.5 text-[12px] text-fg-4">暂无会话，可点击「New Agent」创建。</p>
        )}

        {pinned.length > 0 && (
          <>
            <SecHead icon={<Pin size={10} />} label="PINNED" />
            {pinned.map((s) => (
              <SessionRow key={s.id} s={s} />
            ))}
          </>
        )}

        <SecHead icon={<Folder size={10} />} label="CHAT FOLDERS">
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
            <button
              type="button"
              onClick={submitFolder}
              className="shrink-0 text-[11px] text-acc"
            >
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
              <div
                role="button"
                tabIndex={0}
                onClick={() => toggleCollapsed(g.key)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') toggleCollapsed(g.key);
                }}
                className="group/ws mx-1 flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[12px] text-fg-2 transition-colors hover:bg-shell-hover"
              >
                {isCollapsed ? (
                  <ChevronRight size={11} className="shrink-0 text-fg-4" />
                ) : (
                  <ChevronDown size={11} className="shrink-0 text-fg-4" />
                )}
                <Folder size={13} className="shrink-0 text-fg-3" />
                <span className="min-w-0 flex-1 truncate">{g.label}</span>
                {g.folderId && (
                  <button
                    type="button"
                    title="删除文件夹"
                    aria-label="删除文件夹"
                    onClick={(e) => {
                      e.stopPropagation();
                      void removeFolder(g.folderId as string);
                    }}
                    className="flex h-5 w-5 items-center justify-center rounded text-fg-3 opacity-0 transition-opacity hover:bg-danger-bg group-hover/ws:opacity-100"
                  >
                    <Trash2 size={10} />
                  </button>
                )}
              </div>
              {!isCollapsed && (
                <>
                  {visible.map((s) => (
                    <SessionRow key={s.id} s={s} indented />
                  ))}
                  {overflow > 0 && (
                    <button
                      type="button"
                      onClick={() => toggleShowAll(g.key)}
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

      {/* 工作区选择器:贴在用户卡上方,展开时把自己顶上去、面板落在空隙里 */}
      <WorkspacePicker />

      {/* foot:用户卡(占位)+ 设置齿轮 */}
      <div className="flex shrink-0 items-center gap-2 border-t border-edge p-2">
        <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-acc text-[12px] text-fg-inv">
          本
        </span>
        <span className="flex min-w-0 flex-1 flex-col leading-tight">
          <span className="truncate text-[12px] text-fg">{activeWorkspace?.name ?? '我的空间'}</span>
          <span
            title={activeWorkspace ? displayRoot(activeWorkspace.root) : undefined}
            className="truncate text-[10.5px] text-fg-4"
          >
            {activeWorkspace ? displayRoot(activeWorkspace.root) : '本地用户'}
          </span>
        </span>
        <IBtn title="设置" testId="sidebar-settings" onClick={() => openSettings('settings')}>
          <Settings size={13} />
        </IBtn>
      </div>
    </div>
  );
}
