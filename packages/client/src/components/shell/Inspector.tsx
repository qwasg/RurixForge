import { useCallback, useEffect, useState } from 'react';
import { ChevronDown, ChevronRight, File, FileText, Folder, FolderTree, Search, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { apiGet, apiWorkspaceFile, ForgeApiError, type WorkspaceFileResp } from '@/lib/forgeApi';
import { useToastStore } from '@/lib/toastStore';
import { SecHead } from './primitives';

/**
 * F7 wave.5 Inspector 工作区树(参考 ui/inspector.rs 规格;agentd GET /api/forge/workspace/tree 面):
 * 搜索框本地过滤(name/relPath 子串,大小写不敏感)+ 懒加载目录树(chevron 展开,
 * pl=12+depth×12,目录 folder 图标/文件 file 图标,隐藏文件 text_3 降档);
 * 截断(truncated=true)如实标记行;加载/越界错误 toast 如实。
 * F8 wave.1:文件行点击 = 真实只读预览面板(GET /api/forge/workspace/file;monospace 渲染,
 * 400 PATH_OUTSIDE_ROOT/404 PATH_NOT_FOUND/413 FILE_TOO_LARGE/415 BINARY_FILE 错误态如实)。
 *
 * 差异留痕:参考有 git 状态徽章(M/A/D/U)与 branch 胶囊——本仓无 git 面,不落;
 * Subagents 区/当前 run 控制参考在 Inspector 内,本仓子代理面在对话列,不重复落。
 */

export interface WsEntry {
  name: string;
  kind: 'dir' | 'file';
  relPath: string;
  size: number;
  modifiedAt: string;
  hidden: boolean;
}

interface TreeResp {
  path: string;
  entries: WsEntry[];
  total: number;
  truncated: boolean;
}

export default function Inspector() {
  const [query, setQuery] = useState('');
  const [children, setChildren] = useState<Record<string, WsEntry[]>>({});
  const [truncatedDirs, setTruncatedDirs] = useState<Record<string, boolean>>({});
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [loading, setLoading] = useState<Record<string, boolean>>({});
  const [rootLoaded, setRootLoaded] = useState(false);
  const [rootError, setRootError] = useState(false);
  const [previewPath, setPreviewPath] = useState<string | null>(null);

  const loadDir = useCallback(async (relPath: string) => {
    setLoading((m) => ({ ...m, [relPath]: true }));
    try {
      const r = await apiGet<TreeResp>(`/api/forge/workspace/tree?path=${encodeURIComponent(relPath)}`);
      setChildren((m) => ({ ...m, [relPath]: r.entries }));
      setTruncatedDirs((m) => ({ ...m, [relPath]: r.truncated }));
      if (relPath === '') setRootLoaded(true);
    } catch (err) {
      if (relPath === '') setRootError(true);
      useToastStore
        .getState()
        .push('error', `工作区目录加载失败(${relPath === '' ? '根' : relPath}):${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setLoading((m) => ({ ...m, [relPath]: false }));
    }
  }, []);

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
            onClick={() => setPreviewPath(e.relPath)}
            className={cn(
              'flex w-full items-center gap-[5px] py-[3px] pr-3.5 text-left text-[12.5px] hover:bg-shell-hover',
              e.hidden ? 'text-fg-3' : 'text-fg',
              previewPath === e.relPath && 'bg-shell-selection',
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
    <div data-testid="inspector" className="flex h-full min-h-0 flex-col bg-shell-sidebar">
      <SecHead icon={<FolderTree size={10} />} label="工作区" />
      {/* 搜索框 */}
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
      {/* F8 wave.1:只读文件预览面板 */}
      {previewPath !== null && (
        <FilePreview path={previewPath} onClose={() => setPreviewPath(null)} />
      )}
    </div>
  );
}

/** F8 wave.1 文件预览错误码 → 诚实中文语义(如实,不粉饰)。 */
function previewErrorLabel(err: unknown): string {
  if (err instanceof ForgeApiError) {
    switch (err.code) {
      case 'PATH_OUTSIDE_ROOT':
        return `路径越界(400 PATH_OUTSIDE_ROOT):${err.message}`;
      case 'PATH_NOT_FOUND':
        return `文件不存在(404 PATH_NOT_FOUND):${err.message}`;
      case 'FILE_TOO_LARGE':
        return `文件超 256KB 上限(413 FILE_TOO_LARGE):${err.message}`;
      case 'BINARY_FILE':
        return `二进制文件不支持预览(415 BINARY_FILE):${err.message}`;
      default:
        return `预览失败(${err.status} ${err.code}):${err.message}`;
    }
  }
  return `预览失败:${err instanceof Error ? err.message : String(err)}`;
}

/** F8 wave.1:只读文件预览面板(monospace 渲染;加载/错误态如实)。 */
function FilePreview({ path, onClose }: { path: string; onClose: () => void }) {
  const [data, setData] = useState<WorkspaceFileResp | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    setData(null);
    setError(null);
    setLoading(true);
    apiWorkspaceFile(path)
      .then((r) => {
        if (alive) setData(r);
      })
      .catch((err: unknown) => {
        if (alive) setError(previewErrorLabel(err));
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [path]);

  return (
    <div
      data-testid="ws-preview"
      className="flex max-h-[45%] shrink-0 flex-col border-t border-edge bg-shell-panel"
    >
      <div className="flex h-[26px] shrink-0 items-center gap-1.5 border-b border-edge px-2.5">
        <FileText size={11} className="shrink-0 text-fg-3" />
        <span className="min-w-0 flex-1 truncate text-[11px] text-fg-2" data-testid="ws-preview-name">
          {data ? `${data.name}(${data.size} B)` : path}
        </span>
        <button
          type="button"
          aria-label="关闭预览"
          data-testid="ws-preview-close"
          onClick={onClose}
          className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-fg-3 hover:bg-shell-hover"
        >
          <X size={11} />
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-2.5 py-1.5">
        {loading && (
          <div className="text-[11px] text-fg-4" data-testid="ws-preview-loading">
            加载中…
          </div>
        )}
        {!loading && error !== null && (
          <div className="text-[11px] text-warn" data-testid="ws-preview-error">
            {error}
          </div>
        )}
        {!loading && error === null && data !== null && (
          <pre
            className="whitespace-pre-wrap break-all font-code text-[11px] leading-[1.5] text-fg"
            data-testid="ws-preview-content"
          >
            {data.content}
          </pre>
        )}
      </div>
    </div>
  );
}
