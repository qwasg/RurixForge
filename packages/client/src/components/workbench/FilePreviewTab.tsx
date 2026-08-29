import { useCallback, useEffect, useRef, useState } from 'react';
import { ChevronRight, X } from 'lucide-react';
import {
  apiWorkspaceFile,
  apiWorkspaceFileWrite,
  ForgeApiError,
  type WorkspaceFileResp,
} from '@/lib/forgeApi';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import CodeEditor from './CodeEditor';

/**
 * 工作区文件编辑器 tab(F8 只读预览 → F9 CodeMirror 6 可编辑)。
 * Cursor 编辑器组样式:面包屑 + CM6(行号/高亮/折叠/查找);错误码如实。
 * 保存:Ctrl+S / 头栏保存钮 → PUT workspace/file(baseModifiedAt 乐观并发,409 如实);
 * EOL 保真:CM6 内部 LF 归一,加载嗅探 CRLF、保存还原(混合 EOL 文件保存后统一,如实);
 * 草稿暂存:Workbench 只渲染激活 tab,切走即卸载——drafts 按 tabId 暂存未保存改动,
 * 切回还原(undo 栈不跨挂载,如实不伪造);dirty 经 workbenchStore.setTabDirty 同步 tabbar。
 */

function errorLabel(err: unknown, verb: string): string {
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
      case 'FILE_CONFLICT':
        return `文件已被外部修改(409 FILE_CONFLICT):${err.message}`;
      case 'FORGE_IO':
        return `磁盘 IO 失败(500 FORGE_IO):${err.message}`;
      default:
        return `${verb}失败(${err.status} ${err.code}):${err.message}`;
    }
  }
  return `${verb}失败:${err instanceof Error ? err.message : String(err)}`;
}

export function previewErrorLabel(err: unknown): string {
  return errorLabel(err, '预览');
}

export function saveErrorLabel(err: unknown): string {
  return errorLabel(err, '保存');
}

/** EOL 嗅探(含 \r\n 即视为 CRLF 文件;混合 EOL 保存后统一,如实)。 */
export function sniffEol(content: string): '\r\n' | '\n' {
  return content.includes('\r\n') ? '\r\n' : '\n';
}

/** LF 归一文本 → 原 EOL 还原(保存前调用,Windows 文件不被静默改 EOL)。 */
export function restoreEol(lfText: string, eol: '\r\n' | '\n'): string {
  return eol === '\r\n' ? lfText.replace(/\n/g, '\r\n') : lfText;
}

/** 跨 tab 切换的草稿暂存(激活 tab 独占渲染,卸载不丢未保存改动)。 */
interface DraftEntry {
  /** 当前草稿(LF 归一)。 */
  doc: string;
  /** 最后保存基线(LF 归一;dirty = doc !== baseline)。 */
  baseline: string;
  /** 乐观并发令牌(GET/PUT 的 modifiedAt)。 */
  baseToken: string;
  eol: '\r\n' | '\n';
}
const drafts = new Map<string, DraftEntry>();

/** 测试收尾用:清空草稿暂存。 */
export function clearFileDrafts(): void {
  drafts.clear();
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) {
    const kb = n / 1024;
    return `${kb < 10 ? kb.toFixed(1) : Math.round(kb)} KB`;
  }
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function pathSegments(path: string): string[] {
  return path.replace(/\\/g, '/').split('/').filter(Boolean);
}

type SaveState = 'clean' | 'dirty' | 'saving' | 'error';

export default function FilePreviewTab({ path, tabId }: { path: string; tabId: string }) {
  const closeTab = useWorkbenchStore((st) => st.closeTab);
  const forceCloseTab = useWorkbenchStore((st) => st.forceCloseTab);
  const cancelCloseTab = useWorkbenchStore((st) => st.cancelCloseTab);
  const setTabDirty = useWorkbenchStore((st) => st.setTabDirty);
  const closeConfirm = useWorkbenchStore((st) => st.pendingCloseTabId === tabId);
  const activeWorkspaceId = useWorkspaceStore((st) => st.activeWorkspaceId);
  const [data, setData] = useState<WorkspaceFileResp | null>(null);
  const [size, setSize] = useState<number>(0);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  /** 重载计数(重新加载 = 放弃草稿 + 重挂编辑器;保存成功不重挂,光标/undo 栈不丢)。 */
  const [loadNonce, setLoadNonce] = useState(0);
  const [saveState, setSaveState] = useState<SaveState>('clean');
  const [saveError, setSaveError] = useState<string | null>(null);

  // 编辑会话(ref:变更不触发重渲染;编辑器为文档事实源)。
  const docRef = useRef('');
  const baselineRef = useRef('');
  const baseTokenRef = useRef('');
  const eolRef = useRef<'\r\n' | '\n'>('\n');
  const savingRef = useRef(false);

  useEffect(() => {
    let alive = true;
    setData(null);
    setError(null);
    setLoading(true);
    apiWorkspaceFile(path, activeWorkspaceId)
      .then((r) => {
        if (!alive) return;
        const lf = r.content.replace(/\r\n/g, '\n');
        const restored = drafts.get(tabId);
        if (restored) {
          // 切回还原草稿;baseToken 保留旧值——磁盘若已被外部改写,保存时 409 如实暴露。
          docRef.current = restored.doc;
          baselineRef.current = restored.baseline;
          baseTokenRef.current = restored.baseToken;
          eolRef.current = restored.eol;
        } else {
          docRef.current = lf;
          baselineRef.current = lf;
          baseTokenRef.current = r.modifiedAt;
          eolRef.current = sniffEol(r.content);
        }
        const dirty = docRef.current !== baselineRef.current;
        setSaveState(dirty ? 'dirty' : 'clean');
        setTabDirty(tabId, dirty);
        setSize(r.size);
        setData(r);
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
  }, [path, activeWorkspaceId, tabId, loadNonce, setTabDirty]);

  const handleDocChanged = useCallback(
    (doc: string) => {
      docRef.current = doc;
      const dirty = doc !== baselineRef.current;
      if (dirty) {
        drafts.set(tabId, {
          doc,
          baseline: baselineRef.current,
          baseToken: baseTokenRef.current,
          eol: eolRef.current,
        });
      } else {
        drafts.delete(tabId);
      }
      setSaveState((prev) => (prev === 'saving' ? prev : dirty ? 'dirty' : 'clean'));
      if (dirty) setSaveError(null);
      setTabDirty(tabId, dirty);
    },
    [tabId, setTabDirty],
  );

  /** 保存(Ctrl+S/保存钮/保存并关闭共用);返回 true = 保存后处于干净态。 */
  const save = useCallback(async (): Promise<boolean> => {
    if (savingRef.current) return false;
    const sent = docRef.current;
    if (sent === baselineRef.current) return true;
    savingRef.current = true;
    setSaveState('saving');
    setSaveError(null);
    try {
      const resp = await apiWorkspaceFileWrite(
        path,
        restoreEol(sent, eolRef.current),
        baseTokenRef.current,
        activeWorkspaceId,
      );
      baselineRef.current = sent;
      baseTokenRef.current = resp.modifiedAt;
      setSize(resp.size);
      // 保存期间可能继续输入:按最新草稿重算 dirty。
      const dirty = docRef.current !== baselineRef.current;
      if (dirty) {
        drafts.set(tabId, {
          doc: docRef.current,
          baseline: baselineRef.current,
          baseToken: baseTokenRef.current,
          eol: eolRef.current,
        });
      } else {
        drafts.delete(tabId);
      }
      setSaveState(dirty ? 'dirty' : 'clean');
      setTabDirty(tabId, dirty);
      return !dirty;
    } catch (err) {
      setSaveState('error');
      setSaveError(saveErrorLabel(err));
      return false;
    } finally {
      savingRef.current = false;
    }
  }, [path, activeWorkspaceId, tabId, setTabDirty]);

  // Ctrl+S:编辑器外聚焦时也可保存(编辑器内由 CM keymap 消费,savingRef 防重入)。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 's') {
        e.preventDefault();
        void save();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [save]);

  /** 重新加载:放弃本地草稿,按磁盘现状重挂编辑器(409 冲突恢复口)。 */
  const reload = useCallback(() => {
    drafts.delete(tabId);
    setSaveError(null);
    setLoadNonce((n) => n + 1);
  }, [tabId]);

  const name = data?.name ?? path.replace(/\\/g, '/').split('/').pop() ?? path;
  const crumbs = pathSegments(path);
  const stateLabel =
    saveState === 'saving'
      ? '保存中…'
      : saveState === 'error'
        ? '保存失败'
        : saveState === 'dirty'
          ? '未保存'
          : '已保存';

  return (
    <div data-testid="ws-preview" className="flex h-full min-h-0 flex-col bg-shell-bg">
      <div className="flex h-[22px] shrink-0 items-center gap-1 border-b border-edge px-2.5">
        <nav className="flex min-w-0 flex-1 items-center gap-0.5 overflow-hidden text-[11px]">
          {(crumbs.length > 0 ? crumbs : [name]).map((seg, i, arr) => {
            const last = i === arr.length - 1;
            return (
              <span key={`${seg}-${i}`} className="flex min-w-0 items-center gap-0.5">
                {i > 0 && <ChevronRight size={9} className="shrink-0 text-fg-4" />}
                <span
                  className={last ? 'truncate text-fg' : 'shrink-0 text-fg-3'}
                  data-testid={last ? 'ws-preview-name' : undefined}
                >
                  {last ? (data ? `${name}(${size} B)` : name) : seg}
                </span>
              </span>
            );
          })}
        </nav>
        {data !== null && saveState === 'dirty' && (
          <button
            type="button"
            data-testid="ws-save-btn"
            onClick={() => void save()}
            className="flex h-[16px] shrink-0 items-center rounded border border-edge-strong bg-shell-panel px-1.5 text-[10.5px] text-fg-2 transition-colors hover:bg-shell-hover hover:text-fg"
            title="保存(Ctrl+S)"
          >
            保存
          </button>
        )}
        {data !== null && (
          <span
            data-testid="ws-save-state"
            className={`shrink-0 text-[10.5px] ${saveState === 'error' ? 'text-warn' : saveState === 'dirty' ? 'text-fg-2' : 'text-fg-4'}`}
          >
            {`${formatBytes(size)} · ${stateLabel}`}
          </span>
        )}
        <button
          type="button"
          aria-label="关闭预览"
          data-testid="ws-preview-close"
          onClick={() => closeTab(tabId)}
          className="flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded text-fg-3 hover:bg-shell-hover hover:text-fg"
        >
          <X size={11} />
        </button>
      </div>
      {closeConfirm && (
        <div
          data-testid="ws-close-confirm"
          className="flex shrink-0 items-center gap-2 border-b border-edge bg-shell-sunk px-2.5 py-1 text-[11px]"
        >
          <span className="min-w-0 flex-1 truncate text-fg-2">有未保存的改动,关闭前要保存吗?</span>
          <button
            type="button"
            data-testid="ws-close-save"
            onClick={() => {
              void save().then((ok) => {
                if (ok) {
                  drafts.delete(tabId);
                  forceCloseTab(tabId);
                }
              });
            }}
            className="shrink-0 rounded border border-edge-strong bg-shell-panel px-1.5 py-px text-fg hover:bg-shell-hover"
          >
            保存并关闭
          </button>
          <button
            type="button"
            data-testid="ws-close-discard"
            onClick={() => {
              drafts.delete(tabId);
              forceCloseTab(tabId);
            }}
            className="shrink-0 rounded border border-edge-strong px-1.5 py-px text-warn hover:bg-shell-hover"
          >
            放弃并关闭
          </button>
          <button
            type="button"
            data-testid="ws-close-cancel"
            onClick={cancelCloseTab}
            className="shrink-0 rounded px-1.5 py-px text-fg-3 hover:bg-shell-hover hover:text-fg"
          >
            取消
          </button>
        </div>
      )}
      {saveError !== null && (
        <div
          data-testid="ws-save-error"
          className="flex shrink-0 items-center gap-2 border-b border-edge bg-warn-bg px-2.5 py-1 text-[11px] text-warn"
        >
          <span className="min-w-0 flex-1 truncate">{saveError}</span>
          <button
            type="button"
            data-testid="ws-reload"
            onClick={reload}
            className="shrink-0 rounded border border-edge-strong px-1.5 py-px hover:bg-shell-hover"
          >
            重新加载(放弃本地改动)
          </button>
        </div>
      )}
      <div className="min-h-0 flex-1">
        {loading && (
          <div className="px-3 py-2 text-[11px] text-fg-4" data-testid="ws-preview-loading">
            加载中…
          </div>
        )}
        {!loading && error !== null && (
          <div className="px-3 py-2 text-[11px] text-warn" data-testid="ws-preview-error">
            {error}
          </div>
        )}
        {!loading && error === null && data !== null && (
          <CodeEditor
            key={`${tabId}:${loadNonce}`}
            path={path}
            initialDoc={docRef.current}
            onDocChanged={handleDocChanged}
            onSave={() => void save()}
            data-testid="ws-preview-content"
            className="h-full min-h-0 font-code text-[13px]"
          />
        )}
        {!loading && error === null && data?.truncated === true && (
          <div className="border-t border-edge px-3 py-1 text-[10.5px] text-fg-4">内容已截断(如实)</div>
        )}
      </div>
    </div>
  );
}
