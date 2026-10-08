import { useEffect, useRef } from 'react';
import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { bracketMatching, foldGutter, indentOnInput } from '@codemirror/language';
import { highlightSelectionMatches, searchKeymap } from '@codemirror/search';
import { Compartment, EditorState } from '@codemirror/state';
import {
  EditorView,
  drawSelection,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from '@codemirror/view';
import { langExtensionForPath } from '@/lib/cmLang';
import { forgeEditorTheme } from '@/lib/cmTheme';
import { useEditorAnnotationStore } from '@/lib/editorReferences';
import type { EditorSelection } from '@forge/protocol';

/**
 * F9:CodeMirror 6 代码编辑器(轻量 IDE 面:行号/折叠/查找/括号匹配/undo 栈/Tab 缩进)。
 * 命令式挂载:EditorView 生命周期跟随 path(变更即重建);
 * initialDoc 仅首挂载消费(重载由上层换 key 重挂,避免打字期间被外部覆盖);
 * 语言包经 langExtensionForPath 懒加载 → Compartment 热插(加载失败保持纯文本,编辑不受影响);
 * Mod-s 在编辑器聚焦时触发 onSave(preventDefault 由 keymap 声明)。
 */
export default function CodeEditor({
  path,
  initialDoc,
  readOnly = false,
  onDocChanged,
  onSave,
  onSelectionChanged,
  revealLine,
  className,
  'data-testid': testId,
}: {
  path: string;
  /** 初始文档(LF 归一后;EOL 嗅探/还原由上层负责)。 */
  initialDoc: string;
  readOnly?: boolean;
  onDocChanged?: (doc: string) => void;
  onSave?: () => void;
  onSelectionChanged?: (range: NonNullable<EditorSelection['range']>, text: string) => void;
  revealLine?: { line: number; token: number } | null;
  className?: string;
  'data-testid'?: string;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  // 回调走 ref:handler 身份变化不重建编辑器(重建会丢 undo 栈与光标)。
  const onDocChangedRef = useRef(onDocChanged);
  const onSaveRef = useRef(onSave);
  const selectionCallback = useRef(onSelectionChanged);
  selectionCallback.current = onSelectionChanged;
  const reveal = useEditorAnnotationStore((s) => s.reveal);
  onDocChangedRef.current = onDocChanged;
  onSaveRef.current = onSave;
  const initialDocRef = useRef(initialDoc);
  initialDocRef.current = initialDoc;
  const readOnlyRef = useRef(readOnly);
  readOnlyRef.current = readOnly;
  const readOnlyComp = useRef(new Compartment());
  const langComp = useRef(new Compartment());

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const view = new EditorView({
      state: EditorState.create({
        doc: initialDocRef.current,
        extensions: [
          lineNumbers(),
          highlightActiveLineGutter(),
          foldGutter(),
          drawSelection(),
          history(),
          indentOnInput(),
          bracketMatching(),
          closeBrackets(),
          highlightActiveLine(),
          highlightSelectionMatches(),
          keymap.of([
            {
              key: 'Mod-s',
              preventDefault: true,
              run: () => {
                onSaveRef.current?.();
                return true;
              },
            },
            ...closeBracketsKeymap,
            ...defaultKeymap,
            ...searchKeymap,
            ...historyKeymap,
            indentWithTab,
          ]),
          forgeEditorTheme(),
          readOnlyComp.current.of([
            EditorState.readOnly.of(readOnlyRef.current),
            EditorView.editable.of(!readOnlyRef.current),
          ]),
          langComp.current.of([]),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) onDocChangedRef.current?.(u.state.doc.toString());
            if (u.selectionSet || u.docChanged) {
              const { from, to } = u.state.selection.main;
              selectionCallback.current?.({ from, to, startLine: u.state.doc.lineAt(from).number, endLine: u.state.doc.lineAt(to).number }, u.state.doc.sliceString(from, to));
            }
          }),
        ],
      }),
      parent: host,
    });
    viewRef.current = view;
    let alive = true;
    langExtensionForPath(path)
      .then((ext) => {
        if (alive && ext !== null) {
          view.dispatch({ effects: langComp.current.reconfigure(ext) });
        }
      })
      .catch(() => {
        // 语言包加载失败 → 保持纯文本(高亮缺席如实,编辑不受影响)。
      });
    return () => {
      alive = false;
      viewRef.current = null;
      view.destroy();
    };
  }, [path]);

  useEffect(() => {
    const view = viewRef.current;
    const range = reveal?.reference.selection?.range;
    if (!view || reveal?.reference.kind !== 'source' || reveal.reference.path !== path || !range) return;
    const from = Math.min(view.state.doc.length, range.from), to = Math.min(view.state.doc.length, range.to);
    view.dispatch({ selection: { anchor: from, head: to }, effects: EditorView.scrollIntoView(from, { y: 'center' }) });
    view.focus();
  }, [reveal, path]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || !revealLine) return;
    const line = view.state.doc.line(Math.max(1, Math.min(view.state.doc.lines, revealLine.line)));
    view.dispatch({ selection: { anchor: line.from, head: line.to }, effects: EditorView.scrollIntoView(line.from, { y: 'center' }) });
  }, [revealLine, path]);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: readOnlyComp.current.reconfigure([
        EditorState.readOnly.of(readOnly),
        EditorView.editable.of(!readOnly),
      ]),
    });
  }, [readOnly]);

  return <div ref={hostRef} data-testid={testId} className={className} />;
}
