/**
 * 全局快捷键单一事实源:Shell 全局监听、命令注册表、标题栏菜单/按钮提示与快捷键弹窗都读这里,
 * 改键只改一处。编辑器域(视口 W/E/R、文件编辑器 Ctrl+S 等)由各自组件监听,这里只登记展示。
 */
export const KEYS = {
  palette: 'Ctrl+K',
  newSession: 'Ctrl+Shift+N',
  toggleSessions: 'Ctrl+B',
  toggleInspector: 'Ctrl+Alt+B',
  toggleBottom: 'Ctrl+J',
  focusSessionSearch: '/',
  closeOverlays: 'Esc',
  saveFile: 'Ctrl+S',
} as const;

export interface ShortcutGroup {
  title: string;
  rows: Array<[label: string, keys: string]>;
}

/** 快捷键弹窗分组;发送键随设置·Agent 页「Ctrl+Enter 发送」开关变化。 */
export function shortcutGroups(submitCtrlEnter: boolean): ShortcutGroup[] {
  return [
    {
      title: '通用',
      rows: [
        ['命令面板 / 搜索', KEYS.palette],
        ['新建会话', KEYS.newSession],
        ['聚焦会话搜索', KEYS.focusSessionSearch],
        ['关闭浮层', KEYS.closeOverlays],
      ],
    },
    {
      title: '视图',
      rows: [
        ['切换会话栏', KEYS.toggleSessions],
        ['切换右栏', KEYS.toggleInspector],
        ['切换底部面板', KEYS.toggleBottom],
      ],
    },
    {
      title: '对话',
      rows: submitCtrlEnter
        ? [
            ['发送', 'Ctrl+Enter'],
            ['换行', 'Enter'],
          ]
        : [
            ['发送', 'Enter'],
            ['换行', 'Shift+Enter'],
          ],
    },
    {
      title: '文件编辑器',
      rows: [
        ['保存', KEYS.saveFile],
        ['复制 / 粘贴', 'Ctrl+C / V'],
        ['全选', 'Ctrl+A'],
      ],
    },
  ];
}

/** 焦点在可编辑元素里时,单键快捷键(如 /)不应抢输入。 */
export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
}
