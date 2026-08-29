import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import type { Extension } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';

/**
 * F9:文件编辑器主题(CodeMirror 6)。
 * 所有颜色值一律写 CSS var(--*):StyleModule 注入的是真 CSS,var 由浏览器解析,
 * 主题切换(data-theme / themeStore 注入)时编辑器自动跟随,零重建。
 * 高亮 token 挂 theme.css 的 --code-* 双静态表(不接 applyPalette 自定义派色,留痕见 spec)。
 */

/** 语法高亮映射(lezer tag → --code-* 变量)。 */
export const forgeHighlightStyle = HighlightStyle.define([
  {
    tag: [t.keyword, t.moduleKeyword, t.controlKeyword, t.operatorKeyword, t.definitionKeyword, t.self],
    color: 'var(--code-keyword)',
  },
  {
    tag: [t.string, t.special(t.string), t.character, t.docString],
    color: 'var(--code-string)',
  },
  {
    tag: [t.comment, t.lineComment, t.blockComment, t.docComment],
    color: 'var(--code-comment)',
    fontStyle: 'italic',
  },
  {
    tag: [t.number, t.integer, t.float, t.bool, t.null, t.atom],
    color: 'var(--code-number)',
  },
  {
    tag: [t.function(t.variableName), t.function(t.propertyName), t.macroName, t.labelName],
    color: 'var(--code-func)',
  },
  {
    tag: [t.typeName, t.className, t.namespace, t.standard(t.typeName)],
    color: 'var(--code-type)',
  },
  {
    tag: [t.propertyName, t.attributeName, t.definition(t.propertyName)],
    color: 'var(--code-prop)',
  },
  {
    tag: [t.operator, t.punctuation, t.bracket, t.separator, t.derefOperator],
    color: 'var(--code-punct)',
  },
  {
    tag: [t.meta, t.annotation, t.processingInstruction],
    color: 'var(--code-meta)',
  },
  { tag: t.invalid, color: 'var(--danger)' },
  // markdown 面
  { tag: t.heading, color: 'var(--code-keyword)', fontWeight: '600' },
  { tag: [t.link, t.url], color: 'var(--info)', textDecoration: 'underline' },
  { tag: t.emphasis, fontStyle: 'italic' },
  { tag: t.strong, fontWeight: '600' },
  { tag: t.strikethrough, textDecoration: 'line-through' },
]);

/** 编辑器壳样式(gutter/选区/光标/查找面板;字号沿用 --code-size 语义,行高 18px 同旧预览)。 */
export function forgeEditorTheme(): Extension {
  return [
    EditorView.theme({
      '&': {
        backgroundColor: 'var(--bg)',
        color: 'var(--text)',
        fontSize: '13px',
        height: '100%',
      },
      '.cm-scroller': {
        fontFamily: 'var(--font-mono)',
        lineHeight: '18px',
      },
      '.cm-content': {
        caretColor: 'var(--accent)',
        paddingTop: '12px',
        paddingBottom: '12px',
      },
      '&.cm-focused': { outline: 'none' },
      '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--accent)' },
      '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection':
        { backgroundColor: 'var(--bg-selection)' },
      '.cm-activeLine': { backgroundColor: 'var(--bg-hover)' },
      '.cm-gutters': {
        backgroundColor: 'var(--bg)',
        color: 'var(--text-4)',
        borderRight: '1px solid var(--line)',
        fontSize: '12px',
      },
      '.cm-lineNumbers .cm-gutterElement': { paddingLeft: '10px', paddingRight: '8px' },
      '.cm-activeLineGutter': { backgroundColor: 'var(--bg-hover)', color: 'var(--text-2)' },
      '.cm-foldGutter .cm-gutterElement': { color: 'var(--text-4)' },
      '.cm-matchingBracket, &.cm-focused .cm-matchingBracket': {
        backgroundColor: 'var(--accent-bg)',
        outline: '1px solid var(--accent-ring)',
      },
      '.cm-nonmatchingBracket': { color: 'var(--danger)' },
      '.cm-selectionMatch': { backgroundColor: 'var(--accent-bg)' },
      '.cm-searchMatch': {
        backgroundColor: 'var(--warn-bg)',
        outline: '1px solid var(--warn)',
      },
      '.cm-searchMatch.cm-searchMatch-selected': { backgroundColor: 'var(--accent-bg)' },
      '.cm-panels': {
        backgroundColor: 'var(--bg-sunk)',
        color: 'var(--text)',
        fontSize: '12px',
      },
      '.cm-panels.cm-panels-bottom': { borderTop: '1px solid var(--line)' },
      '.cm-panel.cm-search input, .cm-panel.cm-search button': {
        fontSize: '12px',
        fontFamily: 'var(--font-sans)',
      },
      '.cm-textfield': {
        backgroundColor: 'var(--bg-input)',
        border: '1px solid var(--line-strong)',
        color: 'var(--text)',
      },
      '.cm-button': {
        backgroundImage: 'none',
        backgroundColor: 'var(--bg-panel)',
        border: '1px solid var(--line-strong)',
        color: 'var(--text)',
      },
      '.cm-tooltip': {
        backgroundColor: 'var(--bg-float)',
        border: '1px solid var(--line-strong)',
        color: 'var(--text)',
      },
    }),
    syntaxHighlighting(forgeHighlightStyle),
  ];
}
