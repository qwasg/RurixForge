import { useCallback, useEffect, useState } from 'react';
import { ChevronDown, ChevronRight, File, Folder, Search } from 'lucide-react';
import { cn } from '@/lib/cn';
import { apiWorkspaceTree, type WsEntry } from '@/lib/forgeApi';
import { fileTabId, useWorkbenchStore } from '@/lib/workbenchStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useToastStore } from '@/lib/toastStore';

/**
 * F7 wave.5 Inspector 工作区树(参考 ui/inspector.rs 规格;agentd GET /api/forge/workspace/tree 面):
 * 搜索框本地过滤(name/relPath 子串,大小写不敏感)+ 懒加载目录树(chevron 展开,
 * pl=12+depth×12,目录 folder 图标/文件 file 图标,隐藏文件 text_3 降档);
 * 截断(truncated=true)如实标记行;加载/越界错误 toast 如实。
 * F8 wave.1:文件行点击 = 真实只读预览(GET /api/forge/workspace/file;monospace 渲染,
 * 400 PATH_OUTSIDE_ROOT/404 PATH_NOT_FOUND/413 FILE_TOO_LARGE/415 BINARY_FILE 错误态如实)。
 * 预览落个人工作区 tab(Cursor 编辑器组),不在 Inspector 底栏。
 *
 * 差异留痕:参考有 git 状态徽章(M/A/D/U)与 branch 胶囊——本仓无 git 面,不落;
 * Subagents 区/当前 run 控制参考在 Inspector 内,本仓子代理面在对话列,不重复落。
 */

export default function Inspector() {
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

  // 本地过滤:name/relPath 子串;目录保留条件 = 自身命中或已加载后代有命中。
  const q = query.trim().toLowerCase().replace(/\\/g, '/');
  const visible = (e: WsEntry): boolean => {
    if (q === '') return true;
    const selfHit =
      e.name.toLowerCase().includes(q) || e.relPath.toLowerCase().includes(q);
    if (selfHit) return true;
    if (e.kind !== 'dir') return false;
    const kids = children[e.relPath] ?? [];
    return kids.some(visible);
  };

  const renderRows = (relPath: string, depth: number): React.ReactNode[] => {
    const out: React.ReactNode[] = [];
    const entries = (children[relPath] ?? []).filter(visible);
    for (const e of entries) {
      const indent = 12 + depth * 12;
      if (e.kind === 'dir') {
        const isOpen = expanded[e.relPath] === true;
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
          </button>,
        );
        if (isOpen) {
          if (loading[e.relPath]) {
            out.push(
              <div key={`${e.relPath}:loading`} className="py-0.5 text-[11px] text-fg-4" style={{ paddingLeft: 12 + (depth + 1) * 12 }}>
                加载中…
              </div>,
            );
          } else {
            out.push(...renderRows(e.relPath, depth + 1));
            if (truncatedDirs[e.relPath]) {
              out.push(
                <div
                  key={`${e.relPath}:trunc`}
                  data-testid={`ws-truncated-${e.relPath}`}
                  className="py-0.5 text-[10.5px] text-fg-4"
                  style={{ paddingLeft: 12 + (depth + 1) * 12 }}
                >
                  …条目超 500,已截断(如实)
                </div>,
              );
            }
          }
        }
      } else {
        out.push(
          <button
            key={e.relPath}
            type="button"
            data-testid={`ws-file-${e.relPath}`}
            onClick={() => openFile(e.relPath)}
            className={cn(
              'flex w-full items-center gap-[5px] py-[3px] pr-3.5 text-left text-[12.5px] hover:bg-shell-hover',
              e.hidden ? 'text-fg-3' : 'text-fg',
              activeTabId === fileTabId(e.relPath) && 'bg-shell-selection',
            )}
            style={{ paddingLeft: indent }}
          >
            <span className="w-[10px] shrink-0" />
            <File size={12} className="shrink-0 text-fg-4" />
            <span className="min-w-0 flex-1 truncate">{e.name}</span>
          </button>,
        );
      }
    }
    return out;
  };

  const rootEntries = (children[''] ?? []).filter(visible);
  const hasQuery = q !== '';

  return (
    <div data-testid="inspector" className="flex min-h-0 flex-1 flex-col bg-shell-sidebar">
      {/* 搜索框(「工作区」标题由右栏 tab 条承担,见 RightPane) */}
      <div className="px-2.5 pb-1.5">
        <div className="flex h-[30px] items-center gap-1.5 rounded-md border border-edge bg-shell-panel px-2">
          <Search size={12} className="shrink-0 text-fg-4" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索工作区…"
            data-testid="ws-search"
            className="h-full min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-4"
          />
        </div>
      </div>
      {/* 树 */}
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto" data-testid="ws-tree">
        {rootEntries.length === 0 && (
          <div className="px-3.5 py-1.5 text-[12px] text-fg-4" data-testid="ws-empty">
            {hasQuery ? '无匹配结果' : rootError ? '工作区加载失败' : '加载工作区…'}
          </div>
        )}
        {renderRows('', 0)}
        {truncatedDirs[''] && rootEntries.length > 0 && (
          <div className="px-3.5 py-0.5 text-[10.5px] text-fg-4" data-testid="ws-truncated-root">
            …条目超 500,已截断(如实)
          </div>
        )}
      </div>
    </div>
  );
}
