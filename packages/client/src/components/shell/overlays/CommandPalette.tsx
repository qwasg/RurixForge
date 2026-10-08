import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { File, MessageSquareText, Search, TerminalSquare } from 'lucide-react';
import { cn } from '@/lib/cn';
import { filterCommands, SECTION_LABELS, type Command, type CommandSection } from '@/lib/commands';
import { apiWorkspaceSearch, type WorkspaceSearchHit } from '@/lib/forgeApi';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSessionStore, type ForgeSession } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { relativeTime } from '../Sidebar';
import { Kbd } from '../primitives';

/**
 * 命令面板(Ctrl+K / 标题栏搜索胶囊;D-040 起真能搜会话与文件):
 * 结果三组 = 会话(本地按标题匹配,空查询列最近 5 个)· 文件(agentd workspace/search,
 * 输入防抖 150ms,打开面板即预热候选缓存)· 命令(注册表,按 Agent/导航/视图 小节)。
 * 选中项按 key 追踪,文件结果异步到达时不会把光标顶走;↑↓ 循环 / Enter 打开 / Esc 关。
 */

type Item =
  | { kind: 'session'; key: string; session: ForgeSession }
  | { kind: 'file'; key: string; hit: WorkspaceSearchHit }
  | { kind: 'command'; key: string; cmd: Command };

interface FileState {
  query: string;
  hits: WorkspaceSearchHit[];
  total: number;
  loading: boolean;
  error: string | null;
}

const IDLE_FILES: FileState = { query: '', hits: [], total: 0, loading: false, error: null };
const SEARCH_DEBOUNCE_MS = 150;

function GroupHead({ label, aside }: { label: string; aside?: ReactNode }) {
  return (
    <div className="flex items-center px-3.5 pb-0.5 pt-2 text-[10px] font-semibold uppercase text-fg-4">
      <span>{label}</span>
      {aside !== undefined && <span className="ml-auto font-normal normal-case">{aside}</span>}
    </div>
  );
}

export default function CommandPalette() {
  const open = useOverlayStore((st) => st.palette);
  const close = useOverlayStore((st) => st.close);
  const sessions = useSessionStore((st) => st.sessions);
  const workspaces = useWorkspaceStore((st) => st.workspaces);
  const workspaceId = useWorkspaceStore((st) => st.activeWorkspaceId);
  const [query, setQuery] = useState('');
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [files, setFiles] = useState<FileState>(IDLE_FILES);
  const inputRef = useRef<HTMLInputElement>(null);
  const rowRefs = useRef(new Map<string, HTMLButtonElement>());

  useEffect(() => {
    if (!open) return;
    setQuery('');
    setSelectedKey(null);
    setFiles(IDLE_FILES);
    // 等一帧再 focus,确保已挂载
    requestAnimationFrame(() => inputRef.current?.focus());
    // 预热文件候选缓存:大工作区首次列文件在秒级,别让第一次敲键干等
    void apiWorkspaceSearch('', workspaceId, 1).catch(() => {});
  }, [open, workspaceId]);

  const q = query.trim();
  const qLower = q.toLowerCase();

  // 文件搜索(防抖;过期响应丢弃)
  useEffect(() => {
    if (!open || q === '') {
      setFiles(IDLE_FILES);
      return;
    }
    setFiles((f) => ({ ...f, query: q, loading: true, error: null }));
    let cancelled = false;
    const timer = setTimeout(() => {
      apiWorkspaceSearch(q, workspaceId, 8)
        .then((r) => {
          if (!cancelled) setFiles({ query: q, hits: r.results, total: r.total, loading: false, error: null });
        })
        .catch((err: unknown) => {
          if (!cancelled) {
            setFiles({ query: q, hits: [], total: 0, loading: false, error: err instanceof Error ? err.message : String(err) });
          }
        });
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [open, q, workspaceId]);

  const workspaceName = useMemo(() => new Map(workspaces.map((w) => [w.id, w.name] as const)), [workspaces]);

  const sessionItems = useMemo<Item[]>(() => {
    const list =
      qLower === ''
        ? [...sessions].sort((a, b) => (Date.parse(b.updatedAt) || 0) - (Date.parse(a.updatedAt) || 0)).slice(0, 5)
        : sessions
            .filter((s) => (s.title || s.id).toLowerCase().includes(qLower) || s.id.toLowerCase().includes(qLower))
            .slice(0, 6);
    return list.map((s) => ({ kind: 'session', key: `session:${s.id}`, session: s }));
  }, [sessions, qLower]);

  const fileItems = useMemo<Item[]>(
    () => (files.query === q ? files.hits.map((hit) => ({ kind: 'file' as const, key: `file:${hit.path}`, hit })) : []),
    [files, q],
  );

  const commandItems = useMemo<Item[]>(
    () => filterCommands(query).map((cmd) => ({ kind: 'command', key: `command:${cmd.id}`, cmd })),
    [query],
  );

  const items = useMemo(() => [...sessionItems, ...fileItems, ...commandItems], [sessionItems, fileItems, commandItems]);
  // 查询刚变、防抖还没发出去时也算「搜索中」,免得先闪一下「没有匹配」
  const filesPending = q !== '' && (files.loading || files.query !== q);
  const selectedIndex = Math.max(0, items.findIndex((i) => i.key === selectedKey));
  const selected = items[selectedIndex];

  useEffect(() => {
    if (selected) rowRefs.current.get(selected.key)?.scrollIntoView?.({ block: 'nearest' });
  }, [selected]);

  if (!open) return null;

  const run = (item: Item) => {
    close('palette');
    if (item.kind === 'command') {
      item.cmd.run();
      return;
    }
    const wb = useWorkbenchStore.getState();
    if (item.kind === 'file') {
      wb.openFile(item.hit.path);
      return;
    }
    const s = item.session;
    // 选中的会话属于别的工作区时,侧栏过滤跟着切过去,免得会话在列表里「消失」
    const ws = useWorkspaceStore.getState();
    if (ws.activeWorkspaceId !== null && (s.workspaceId ?? null) !== ws.activeWorkspaceId) {
      ws.setActive(s.workspaceId ?? null);
    }
    useSessionStore.getState().select(s.id);
    if (wb.tabs.length > 0 && wb.collapsed.chat) wb.togglePane('chat');
  };

  const move = (delta: number) => {
    if (items.length === 0) return;
    const next = (selectedIndex + delta + items.length) % items.length;
    setSelectedKey(items[next].key);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      move(1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      move(-1);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (selected) run(selected);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      close('palette');
    }
  };

  const rowClass = (item: Item) =>
    cn(
      'flex w-full items-center gap-2.5 px-3.5 py-[7px] text-left text-[13px] text-fg-2',
      item.key === selected?.key && 'text-fg',
    );
  const rowStyle = (item: Item) => (item.key === selected?.key ? { background: 'var(--accent-bg)' } : undefined);
  const bindRow = (item: Item) => ({
    ref: (el: HTMLButtonElement | null) => {
      if (el) rowRefs.current.set(item.key, el);
      else rowRefs.current.delete(item.key);
    },
    onMouseEnter: () => setSelectedKey(item.key),
    onClick: () => run(item),
    className: rowClass(item),
    style: rowStyle(item),
  });

  // 命令分组小节:按 section 首见处插入
  let lastSection: CommandSection | '' = '';

  return (
    <div
      data-testid="command-palette-scrim"
      className="absolute inset-0 z-50 flex justify-center"
      style={{ background: 'var(--scrim-palette)' }}
      onMouseDown={() => close('palette')}
    >
      <div
        role="dialog"
        aria-label="命令面板"
        className="forge-pop-in mt-[14vh] flex h-auto w-[620px] max-w-[92vw] flex-col self-start overflow-hidden rounded-xl border border-edge-strong bg-shell-float shadow-float"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-edge px-3.5 py-3">
          <Search size={14} className="shrink-0 text-fg-3" />
          <input
            ref={inputRef}
            value={query}
            data-testid="command-palette-input"
            onChange={(e) => {
              setQuery(e.target.value);
              setSelectedKey(null);
            }}
            onKeyDown={onKeyDown}
            placeholder="搜索会话、文件、命令…"
            className="min-w-0 flex-1 bg-transparent text-[14px] text-fg outline-none placeholder:text-fg-4"
          />
          <Kbd label="Esc" />
        </div>
        <div className="flex max-h-[min(460px,60vh)] flex-col overflow-y-auto pb-1" data-testid="command-palette-list">
          {sessionItems.length > 0 && (
            <>
              <GroupHead label={q === '' ? '最近会话' : '会话'} />
              {sessionItems.map((item) => {
                if (item.kind !== 'session') return null;
                const s = item.session;
                const ws = s.workspaceId ? workspaceName.get(s.workspaceId) : undefined;
                return (
                  <button key={item.key} type="button" data-testid={`palette-session-${s.id}`} {...bindRow(item)}>
                    <MessageSquareText size={13} className="shrink-0 text-fg-4" />
                    <span className="min-w-0 flex-1 truncate">{s.title === '' ? s.id : s.title}</span>
                    {ws && <span className="max-w-[140px] shrink-0 truncate text-[11px] text-fg-4">{ws}</span>}
                    <span className="shrink-0 font-code text-[10px] text-fg-4">{relativeTime(s.updatedAt)}</span>
                  </button>
                );
              })}
            </>
          )}

          {q !== '' && (
            <>
              <GroupHead
                label="文件"
                aside={
                  filesPending
                    ? '搜索中…'
                    : files.error === null && files.total > fileItems.length
                      ? `前 ${fileItems.length} / 共 ${files.total} 个`
                      : undefined
                }
              />
              {fileItems.map((item) => {
                if (item.kind !== 'file') return null;
                return (
                  <button key={item.key} type="button" data-testid={`palette-file-${item.hit.path}`} {...bindRow(item)}>
                    <File size={13} className="shrink-0 text-fg-4" />
                    <span className="shrink-0 truncate">{item.hit.name}</span>
                    <span className="min-w-0 flex-1 truncate text-[11px] text-fg-4" dir="rtl">
                      {item.hit.dir === '' ? '' : `\u200E${item.hit.dir}`}
                    </span>
                  </button>
                );
              })}
              {!filesPending && fileItems.length === 0 && (
                <p className="px-3.5 py-1.5 text-[12px] text-fg-4" data-testid="palette-files-empty">
                  {files.error !== null ? `文件搜索不可用:${files.error}` : '没有匹配的文件'}
                </p>
              )}
            </>
          )}

          {commandItems.length > 0 && q !== '' && <GroupHead label="命令" />}
          {commandItems.map((item) => {
            if (item.kind !== 'command') return null;
            const c = item.cmd;
            const head =
              c.section !== lastSection ? (
                <div
                  key={`sec-${c.section}`}
                  className="flex items-center gap-1.5 px-3.5 pb-0.5 pt-2 text-[10px] font-semibold uppercase text-fg-4"
                >
                  {q === '' && <TerminalSquare size={10} />}
                  {SECTION_LABELS[c.section]}
                </div>
              ) : null;
            lastSection = c.section;
            const isSel = item.key === selected?.key;
            return (
              <div key={item.key}>
                {head}
                <button type="button" data-testid={`command-row-${c.id}`} {...bindRow(item)}>
                  <span className="min-w-0 flex-1 truncate">{c.label}</span>
                  {c.shortcut ? <Kbd label={c.shortcut} /> : isSel && <Kbd label="↵" />}
                </button>
              </div>
            );
          })}

          {items.length === 0 && !filesPending && (
            <p className="p-4 text-[12px] text-fg-4">没有匹配的会话、文件或命令</p>
          )}
        </div>
        <div className="flex items-center gap-3 border-t border-edge px-3.5 py-1.5 text-[10.5px] text-fg-4">
          <span className="flex items-center gap-1">
            <Kbd label="↑↓" /> 选择
          </span>
          <span className="flex items-center gap-1">
            <Kbd label="↵" /> 打开
          </span>
          <span className="ml-auto">会话 · 文件 · 命令</span>
        </div>
      </div>
    </div>
  );
}
