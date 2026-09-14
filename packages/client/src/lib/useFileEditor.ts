import { useCallback, useEffect, useRef, useState } from 'react';
import {
  apiWorkspaceFile,
  apiWorkspaceFileWrite,
  ForgeApiError,
  type WorkspaceFileResp,
} from './forgeApi';
import { useWorkbenchStore } from './workbenchStore';
import { useWorkspaceStore } from './workspaceStore';

/**
 * 工作区文本文件的加载/保存会话(F9 文件编辑器的核心,D-035 起与 Plan 页签共用)。
 *
 * 一处实现管住四件容易各写各的事:
 * - 跨 tab 草稿暂存:Workbench 只渲染激活 tab,切走即卸载 —— drafts 按 tabId 存未保存改动;
 * - EOL 保真:CM6 内部 LF 归一,加载嗅探 CRLF、保存还原(混合 EOL 文件保存后统一,如实);
 * - 乐观并发:PUT 带 baseModifiedAt,被外部(含 agent write_file)改过则 409 如实暴露;
 * - dirty 同步:经 workbenchStore.setTabDirty 驱动 tabbar 圆点与关闭拦截。
 */

export type SaveState = 'clean' | 'dirty' | 'saving' | 'error';

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

export interface FileEditorSession {
  /** 加载完成的文件元信息(null = 加载中或失败)。 */
  data: WorkspaceFileResp | null;
  /** 当前磁盘尺寸(保存后刷新)。 */
  size: number;
  loading: boolean;
  /** 加载失败原因(如实错误码文案)。 */
  error: string | null;
  saveState: SaveState;
  saveError: string | null;
  /** 编辑器初始文档(LF 归一;含还原的草稿)。 */
  initialDoc: string;
  /** 重载计数:换 key 重挂 CodeMirror(放弃草稿 + 光标/undo 栈重置)。 */
  loadNonce: number;
  /** 文档变更回调(交给 CodeEditor.onDocChanged)。 */
  onDocChanged: (doc: string) => void;
  /** 保存;返回 true = 保存后处于干净态。 */
  save: () => Promise<boolean>;
  /** 放弃本地草稿按磁盘现状重挂(409 冲突恢复口)。 */
  reload: () => void;
  /** 读当前草稿(不触发渲染;Build 前取全文用)。 */
  currentDoc: () => string;
  dirty: boolean;
}

/**
 * @param path 工作区相对路径
 * @param tabId 承载 tab 的 id(草稿键 + dirty 同步目标)
 * @param externalNonce 外部重载信号(计划被 agent 覆盖时 +1;仅在无未保存改动时生效)
 */
export function useFileEditor(
  path: string,
  tabId: string,
  externalNonce = 0,
): FileEditorSession {
  const setTabDirty = useWorkbenchStore((st) => st.setTabDirty);
  const activeWorkspaceId = useWorkspaceStore((st) => st.activeWorkspaceId);
  const [data, setData] = useState<WorkspaceFileResp | null>(null);
  const [size, setSize] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadNonce, setLoadNonce] = useState(0);
  const [saveState, setSaveState] = useState<SaveState>('clean');
  const [saveError, setSaveError] = useState<string | null>(null);

  // 编辑会话(ref:变更不触发重渲染;编辑器为文档事实源)。
  const docRef = useRef('');
  const baselineRef = useRef('');
  const baseTokenRef = useRef('');
  const eolRef = useRef<'\r\n' | '\n'>('\n');
  const savingRef = useRef(false);

  // 外部重载(agent 覆盖了文件):有未保存改动时不动用户的草稿,只让保存时的 409 说话。
  const prevExternal = useRef(externalNonce);
  useEffect(() => {
    if (externalNonce === prevExternal.current) return;
    prevExternal.current = externalNonce;
    if (docRef.current !== baselineRef.current) return;
    drafts.delete(tabId);
    setLoadNonce((n) => n + 1);
  }, [externalNonce, tabId]);

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

  const onDocChanged = useCallback(
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

  const reload = useCallback(() => {
    drafts.delete(tabId);
    setSaveError(null);
    setLoadNonce((n) => n + 1);
  }, [tabId]);

  const currentDoc = useCallback(() => docRef.current, []);

  return {
    data,
    size,
    loading,
    error,
    saveState,
    saveError,
    initialDoc: docRef.current,
    loadNonce,
    onDocChanged,
    save,
    reload,
    currentDoc,
    dirty: saveState === 'dirty',
  };
}
