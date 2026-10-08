import { useCallback, useEffect, useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, Folder, GitCompare, RotateCw, Search } from 'lucide-react';
import { cn } from '@/lib/cn';
import { fileIconFor } from '@/lib/fileIcon';
import { apiWorkspaceTree, type GitFileEntry, type GitFileStatus, type WsEntry } from '@/lib/forgeApi';
import {
  buildGitIndex,
  GIT_STATUS_LABEL,
  GIT_STATUS_TONE,
  gitStatusOf,
  useGitPolling,
  useGitStore,
} from '@/lib/gitStore';
import { useThemeStore, type DiffMarkers } from '@/lib/themeStore';
import { fileTabId, useWorkbenchStore } from '@/lib/workbenchStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useToastStore } from '@/lib/toastStore';

/**
 * Inspector 工作区树(agentd GET /api/forge/workspace/tree 面):
 * 搜索框本地过滤 + 懒加载目录树(chevron 展开;隐藏文件 text_3 降档;截断如实标记);
 * 文件行点击 = 工作台文件 tab(F8/F9)。
 *
 * D-040:按扩展名的文件类型图标;git 改动标记(gitStore,agentd workspace/git)——
 * 文件行 M/A/D/U/R/C 字母,含改动的目录行画点;外观设置「差异标记」决定呈现:
 * Color = 文件名按状态着色 + 字母,+/- = 每文件增删行数。搜索框旁「仅看改动」切到改动平铺列表
 * (状态栏分支段点进来即打开),刷新钮重拉已展开的目录与 git 状态。
 * 工作区目录整体未被 git 跟踪时不逐一标记(如实提示),非仓库时改动开关禁用。
 */

function GitMark({
  status,
  entry,
  mode,
}: {
  status: GitFileStatus | null;
  entry: GitFileEntry | undefined;
  mode: DiffMarkers;
}) {
  if (status === null) return null;
  if (mode === 'plusminus' && entry && (entry.insertions !== null || entry.deletions !== null)) {
    return (
      <span className="flex shrink-0 items-center gap-1 font-code text-[10px]" title={GIT_STATUS_LABEL[status]}>
        <span className="text-sage">+{entry.insertions ?? 0}</span>
        <span className="text-danger">−{entry.deletions ?? 0}</span>
      </span>
    );
  }
  return (
    <span
      data-testid="ws-git-mark"
      title={GIT_STATUS_LABEL[status]}
      className={cn('w-3 shrink-0 text-center font-code text-[10px] font-semibold', GIT_STATUS_TONE[status])}
    >
      {status}
    </span>
  );
}

export default function Inspector() {
  useGitPolling();
  const [query, setQuery] = useState('');
  const [children, setChildren] = useState<Record<string, WsEntry[]>>({});
  const [truncatedDirs, setTruncatedDirs] = useState<Record<string, boolean>>({});
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [loading, setLoading] = useState<Record<string, boolean>>({});
  const [rootLoaded, setRootLoaded] = useState(false);
  const [rootError, setRootError] = useState(false);
  const openFile = useWorkbenchStore((st) => st.openFile);
  const activeTabId = useWorkbenchStore((st) => st.activeTabId);
  const activeWorkspaceId = useWorkspaceStore((st) => st.activeWorkspaceId);
  const git = useGitStore((st) => st.status);
  const changesOnly = useGitStore((st) => st.changesOnly);
  const setChangesOnly = useGitStore((st) => st.setChangesOnly);
  const diffMarkers = useThemeStore((st) => st.diffMarkers);

  const isRepo = git?.isRepo === true;
  const rootUntracked = git?.rootUntracked === true;
  const index = useMemo(() => buildGitIndex(isRepo && !rootUntracked ? git : null), [git, isRepo, rootUntracked]);

  const loadDir = useCallback(
    async (relPath: string) => {
      setLoading((m) => ({ ...m, [relPath]: true }));
      try {
        const r = await apiWorkspaceTree(relPath, activeWorkspaceId);
        setChildren((m) => ({ ...m, [relPath]: r.entries }));
        setTruncatedDirs((m) => ({ ...m, [relPath]: r.truncated }));
        if (relPath === '') setRootLoaded(true);
      } catch (err) {
        if (relPath === '') setRootError(true);
        useToastStore
          .getState()
          .push(
            'error',
            `工作区目录加载失败(${relPath === '' ? '根' : relPath}):${err instanceof Error ? err.message : String(err)}`,
          );
      } finally {
        setLoading((m) => ({ ...m, [relPath]: false }));
      }
    },
    [activeWorkspaceId],
  );

  useEffect(() => {
    setChildren({});
    setTruncatedDirs({});
    setExpanded({});
    setLoading({});
    setRootLoaded(false);
    setRootError(false);
  }, [activeWorkspaceId]);

  // 首挂加载根
  useEffect(() => {
    if (!rootLoaded && !rootError && !loading['']) void loadDir('');
  }, [rootLoaded, rootError, loading, loadDir]);

  const toggleDir = (relPath: string) => {
    const next = !expanded[relPath];
    setExpanded((m) => ({ ...m, [relPath]: next }));
    if (next && !(relPath in children)) void loadDir(relPath);
  };

  /** 重拉根与已展开目录(就地覆盖,不清空不闪)+ git 状态。 */
  const refreshAll = () => {
    setRootError(false);
    for (const dir of ['', ...Object.keys(expanded).filter((k) => expanded[k])]) void loadDir(dir);
    void useGitStore.getState().refresh();
  };

  // 本地过滤:name/relPath 子串;目录保留条件 = 自身命中或已加载后代有命中。
  const q = query.trim().toLowerCase().replace(/\\/g, '/');
  const visible = (e: WsEntry): boolean => {
    if (q === '') return true;
    const selfHit = e.name.toLowerCase().includes(q) || e.relPath.toLowerCase().includes(q);
    if (selfHit) return true;
    if (e.kind !== 'dir') return false;
    const kids = children[e.relPath] ?? [];
    return kids.some(visible);
  };

  const truncNote = (key: string, depth: number) => (
    <div
      key={`${key}:trunc`}
      data-testid={`ws-truncated-${key}`}
      className="py-0.5 text-[10.5px] text-fg-4"
      style={{ paddingLeft: 12 + depth * 12 }}
    >
      仅显示前 500 项
    </div>
  );

  const renderRows = (relPath: string, depth: number): React.ReactNode[] => {
    const out: React.ReactNode[] = [];
    const entries = (children[relPath] ?? []).filter(visible);
    for (const e of entries) {
      const indent = 12 + depth * 12;
      const status = gitStatusOf(index, e.relPath);
      if (e.kind === 'dir') {
        const isOpen = expanded[e.relPath] === true;
        const dirty = index.dirty.has(e.relPath) || status === 'U';
        out.push(
          <button
            key={e.relPath}
            type="button"
            data-testid={`ws-dir-${e.relPath}`}
            onClick={() => toggleDir(e.relPath)}
            className="flex w-full items-center gap-[5px] py-[3px] pr-3.5 text-left text-[12.5px] font-medium text-fg-2 hover:bg-shell-hover"
            style={{ paddingLeft: indent }}
          >
            {isOpen ? (
              <ChevronDown size={10} className="shrink-0 text-fg-4" />
            ) : (
              <ChevronRight size={10} className="shrink-0 text-fg-4" />
            )}
            <Folder size={12} className="shrink-0 text-fg-3" />
            <span className="min-w-0 flex-1 truncate">{e.name}</span>
            {dirty && (
              <span
                data-testid="ws-dir-dirty"
                title={status === 'U' ? '未跟踪目录' : '含未提交改动'}
                className={cn('h-1.5 w-1.5 shrink-0 rounded-full', status === 'U' ? 'bg-sage' : 'bg-warn')}
              />
            )}
          </button>,
        );
        if (isOpen) {
          if (loading[e.relPath] && !(e.relPath in children)) {
            out.push(
              <div key={`${e.relPath}:loading`} className="py-0.5 text-[11px] text-fg-4" style={{ paddingLeft: 12 + (depth + 1) * 12 }}>
                加载中…
              </div>,
            );
          } else {
            out.push(...renderRows(e.relPath, depth + 1));
            if (truncatedDirs[e.relPath]) out.push(truncNote(e.relPath, depth + 1));
          }
        }
      } else {
        const { Icon, tone } = fileIconFor(e.name);
        const nameTone = status !== null && diffMarkers === 'color' ? GIT_STATUS_TONE[status] : undefined;
        out.push(
          <button
            key={e.relPath}
            type="button"
            data-testid={`ws-file-${e.relPath}`}
            data-git={status ?? undefined}
            onClick={() => openFile(e.relPath)}
            className={cn(
              'flex w-full items-center gap-[5px] py-[3px] pr-3.5 text-left text-[12.5px] hover:bg-shell-hover',
              e.hidden ? 'text-fg-3' : 'text-fg',
              activeTabId === fileTabId(e.relPath) && 'bg-shell-selection',
            )}
            style={{ paddingLeft: indent }}
          >
            <span className="w-[10px] shrink-0" />
            <Icon size={12} className={cn('shrink-0', tone)} />
            <span className={cn('min-w-0 flex-1 truncate', nameTone)}>{e.name}</span>
            <GitMark status={status} entry={index.entries.get(e.relPath)} mode={diffMarkers} />
          </button>,
        );
      }
    }
    return out;
  };

  const rootEntries = (children[''] ?? []).filter(visible);
  const hasQuery = q !== '';

  const changed = useMemo(
    () => (git?.files ?? []).filter((f) => q === '' || f.path.toLowerCase().includes(q)),
    [git, q],
  );

  const toolBtn = (on: boolean) =>
    cn(
      'flex h-[30px] w-[26px] shrink-0 items-center justify-center rounded-md border border-transparent text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:opacity-40 disabled:hover:bg-transparent',
      on && 'border-acc-ring bg-acc-bg text-acc hover:bg-acc-bg hover:text-acc',
    );

  return (
    <div data-testid="inspector" className="flex min-h-0 flex-1 flex-col bg-shell-sidebar">
      {/* 搜索框 + 仅看改动 + 刷新(「工作区」标题由右栏 tab 条承担,见 RightPane) */}
      <div className="flex items-center gap-1 px-2.5 pb-1.5">
        <div className="flex h-[30px] min-w-0 flex-1 items-center gap-1.5 rounded-md border border-edge bg-shell-panel px-2 focus-within:border-acc-ring">
          <Search size={12} className="shrink-0 text-fg-4" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={changesOnly ? '筛选改动…' : '搜索工作区…'}
            data-testid="ws-search"
            className="h-full min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-4"
          />
        </div>
        <button
          type="button"
          data-testid="ws-changes-toggle"
          aria-pressed={changesOnly}
          disabled={!isRepo}
          title={!isRepo ? '当前工作区不是 git 仓库' : changesOnly ? '回到文件树' : '仅看改动'}
          onClick={() => setChangesOnly(!changesOnly)}
          className={toolBtn(changesOnly)}
        >
          <GitCompare size={13} />
        </button>
        <button type="button" data-testid="ws-refresh" title="刷新文件树与 git 状态" onClick={refreshAll} className={toolBtn(false)}>
          <RotateCw size={12} />
        </button>
      </div>

      {changesOnly && isRepo ? (
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto" data-testid="ws-changes">
          <div className="flex items-center gap-1.5 px-3.5 pb-1 text-[10.5px] text-fg-4">
            <span>{git?.detached ? 'HEAD' : (git?.branch ?? '?')}</span>
            {!rootUntracked && (
              <>
                <span>· {git?.total ?? 0} 个改动</span>
                {(git?.total ?? 0) > 0 && (
                  <span className="font-code">
                    <span className="text-sage">+{git?.insertions ?? 0}</span>{' '}
                    <span className="text-danger">−{git?.deletions ?? 0}</span>
                  </span>
                )}
              </>
            )}
          </div>
          {rootUntracked ? (
            <p className="px-3.5 py-1.5 text-[12px] leading-relaxed text-fg-4">
              当前工作区目录整体未被 git 跟踪,没有可比较的改动。
            </p>
          ) : changed.length === 0 ? (
            <p className="px-3.5 py-1.5 text-[12px] text-fg-4">
              {hasQuery ? '没有匹配的改动' : '工作区没有未提交的改动'}
            </p>
          ) : (
            changed.map((f) => {
              const name = f.path.split('/').pop() ?? f.path;
              const dir = f.path.includes('/') ? f.path.slice(0, f.path.lastIndexOf('/')) : '';
              const { Icon, tone } = f.dir ? { Icon: Folder, tone: 'text-fg-3' } : fileIconFor(name);
              const openable = !f.dir && f.status !== 'D';
              return (
                <button
                  key={f.path}
                  type="button"
                  data-testid={`ws-change-${f.path}`}
                  disabled={!openable}
                  title={f.origPath ? `${f.origPath} → ${f.path}` : f.path}
                  onClick={() => openFile(f.path)}
                  className={cn(
                    'flex w-full items-center gap-[5px] py-[3px] pl-3.5 pr-3.5 text-left text-[12.5px] text-fg enabled:hover:bg-shell-hover disabled:cursor-default',
                    activeTabId === fileTabId(f.path) && 'bg-shell-selection',
                  )}
                >
                  <Icon size={12} className={cn('shrink-0', tone)} />
                  <span
                    className={cn(
                      'shrink-0 truncate',
                      diffMarkers === 'color' && GIT_STATUS_TONE[f.status],
                      f.status === 'D' && 'line-through',
                    )}
                  >
                    {name}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-[10.5px] text-fg-4">{dir}</span>
                  <GitMark status={f.status} entry={f} mode={diffMarkers} />
                </button>
              );
            })
          )}
          {git?.truncated && <div className="px-3.5 py-0.5 text-[10.5px] text-fg-4">仅显示前 2000 个改动</div>}
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto" data-testid="ws-tree">
          {rootEntries.length === 0 && (
            <div className="px-3.5 py-1.5 text-[12px] text-fg-4" data-testid="ws-empty">
              {hasQuery ? '无匹配结果' : rootError ? '工作区加载失败' : '加载工作区…'}
            </div>
          )}
          {renderRows('', 0)}
          {truncatedDirs[''] && rootEntries.length > 0 && (
            <div className="px-3.5 py-0.5 text-[10.5px] text-fg-4" data-testid="ws-truncated-root">
              仅显示前 500 项
            </div>
          )}
        </div>
      )}
    </div>
  );
}
