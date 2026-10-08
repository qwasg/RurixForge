import { ChevronRight, X } from 'lucide-react';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useFileEditor } from '@/lib/useFileEditor';
import CodeEditor from './CodeEditor';
import { useState } from 'react';
import type { EditorSelection } from '@forge/protocol';
import AnnotationHandle from '../editor/AnnotationHandle';
import { editorReference, makeAnnotation } from '@/lib/editorReferences';
import { publishEditorSelection } from '@/lib/editorSelection';

/**
 * 工作区文件编辑器 tab(F8 只读预览 → F9 CodeMirror 6 可编辑)。
 * Cursor 编辑器组样式:面包屑 + CM6(行号/高亮/折叠/查找);错误码如实。
 * 加载/保存/dirty/草稿/EOL/409 冲突全套纪律在 lib/useFileEditor(D-035 起与 Plan 页签共用);
 * 本组件只负责壳:面包屑、保存钮与状态、dirty 关闭确认条、冲突恢复条。
 */

// 既有导入方沿用本模块路径(测试与其它组件);实现已下沉到 useFileEditor。
export {
  clearFileDrafts,
  previewErrorLabel,
  restoreEol,
  saveErrorLabel,
  sniffEol,
} from '@/lib/useFileEditor';

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

export default function FilePreviewTab({ path, tabId }: { path: string; tabId: string }) {
  const closeTab = useWorkbenchStore((st) => st.closeTab);
  const forceCloseTab = useWorkbenchStore((st) => st.forceCloseTab);
  const cancelCloseTab = useWorkbenchStore((st) => st.cancelCloseTab);
  const closeConfirm = useWorkbenchStore((st) => st.pendingCloseTabId === tabId);
  const ed = useFileEditor(path, tabId);
  const [selection, setSelection] = useState<{ range: NonNullable<EditorSelection['range']>; text: string } | null>(null);

  const name = ed.data?.name ?? path.replace(/\\/g, '/').split('/').pop() ?? path;
  const crumbs = pathSegments(path);
  const stateLabel =
    ed.saveState === 'saving'
      ? '保存中…'
      : ed.saveState === 'error'
        ? '保存失败'
        : ed.saveState === 'dirty'
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
                  {last ? (ed.data ? `${name}(${ed.size} B)` : name) : seg}
                </span>
              </span>
            );
          })}
        </nav>
        <AnnotationHandle label={selection?.text ? `${name}:${selection.range.startLine}-${selection.range.endLine}` : name}
          annotations={[makeAnnotation(editorReference('source', { path, revision: ed.data?.modifiedAt, selection: selection?.text ? { range: selection.range, ...(ed.dirty ? { excerpt: selection.text, dirty: true } : {}) } : undefined }), selection?.text ? `${name}:${selection.range.startLine}-${selection.range.endLine}` : name)]} />
        {ed.data !== null && ed.saveState === 'dirty' && (
          <button
            type="button"
            data-testid="ws-save-btn"
            onClick={() => void ed.save()}
            className="flex h-[16px] shrink-0 items-center rounded border border-edge-strong bg-shell-panel px-1.5 text-[10.5px] text-fg-2 transition-colors hover:bg-shell-hover hover:text-fg"
            title="保存(Ctrl+S)"
          >
            保存
          </button>
        )}
        {ed.data !== null && (
          <span
            data-testid="ws-save-state"
            className={`shrink-0 text-[10.5px] ${ed.saveState === 'error' ? 'text-warn' : ed.saveState === 'dirty' ? 'text-fg-2' : 'text-fg-4'}`}
          >
            {`${formatBytes(ed.size)} · ${stateLabel}`}
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
              void ed.save().then((ok) => {
                if (ok) forceCloseTab(tabId);
              });
            }}
            className="shrink-0 rounded border border-edge-strong bg-shell-panel px-1.5 py-px text-fg hover:bg-shell-hover"
          >
            保存并关闭
          </button>
          <button
            type="button"
            data-testid="ws-close-discard"
            onClick={() => forceCloseTab(tabId)}
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
      {ed.saveError !== null && (
        <div
          data-testid="ws-save-error"
          className="flex shrink-0 items-center gap-2 border-b border-edge bg-warn-bg px-2.5 py-1 text-[11px] text-warn"
        >
          <span className="min-w-0 flex-1 truncate">{ed.saveError}</span>
          <button
            type="button"
            data-testid="ws-reload"
            onClick={ed.reload}
            className="shrink-0 rounded border border-edge-strong px-1.5 py-px hover:bg-shell-hover"
          >
            重新加载(放弃本地改动)
          </button>
        </div>
      )}
      <div className="min-h-0 flex-1">
        {ed.loading && (
          <div className="px-3 py-2 text-[11px] text-fg-4" data-testid="ws-preview-loading">
            加载中…
          </div>
        )}
        {!ed.loading && ed.error !== null && (
          <div className="px-3 py-2 text-[11px] text-warn" data-testid="ws-preview-error">
            {ed.error}
          </div>
        )}
        {!ed.loading && ed.error === null && ed.data !== null && (
          <CodeEditor
            key={`${tabId}:${ed.loadNonce}`}
            path={path}
            initialDoc={ed.initialDoc}
            onDocChanged={ed.onDocChanged}
            onSelectionChanged={(range, text) => { setSelection({ range, text }); publishEditorSelection([editorReference('source', { path, revision: ed.data?.modifiedAt, selection: { range } })]); }}
            onSave={() => void ed.save()}
            data-testid="ws-preview-content"
            className="h-full min-h-0 font-code text-[13px]"
          />
        )}
        {!ed.loading && ed.error === null && ed.data?.truncated === true && (
          <div className="border-t border-edge px-3 py-1 text-[10.5px] text-fg-4">内容已截断(如实)</div>
        )}
      </div>
    </div>
  );
}
