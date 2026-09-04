import { useOverlayStore } from './overlayStore';
import { usePlanStore } from './planStore';
import { useSessionStore } from './sessionStore';
import { useThemeStore } from './themeStore';
import { useToastStore } from './toastStore';
import { useWorkbenchStore } from './workbenchStore';

/**
 * F7 wave.3 命令注册表(参考 app.rs palette_commands;分组 agent/navigate/view)。
 * 命令面板与 TitleBar 菜单共用同一份事实源。
 */

export type CommandSection = 'agent' | 'navigate' | 'view';

export interface Command {
  id: string;
  label: string;
  section: CommandSection;
  shortcut?: string;
  run: () => void;
}

export const COMMANDS: Command[] = [
  {
    id: 'session.new',
    label: 'New Agent',
    section: 'agent',
    shortcut: 'Ctrl+Shift+N',
    run: () => void useSessionStore.getState().create(),
  },
  {
    id: 'tab.editor',
    label: '打开编辑器',
    section: 'navigate',
    run: () => useWorkbenchStore.getState().openEditor(),
  },
  {
    id: 'tab.plan',
    label: '打开 Plan',
    section: 'navigate',
    // D-035:计划是文件,先要有;没有就如实说,不开一个空壳页签。
    run: () => {
      const path = usePlanStore.getState().activePlanPath;
      if (path === null) {
        useToastStore.getState().push('info', '尚无计划:在对话中以 Plan 模式描述任务即可生成');
        return;
      }
      useWorkbenchStore.getState().openPlan(path);
    },
  },
  {
    id: 'tab.todo',
    label: '打开 Todo 看板',
    section: 'navigate',
    run: () => useWorkbenchStore.getState().openTab('todo'),
  },
  {
    id: 'tab.proposals',
    label: '打开提案',
    section: 'navigate',
    run: () => useWorkbenchStore.getState().openTab('proposals'),
  },
  {
    id: 'tab.store',
    label: '打开资产商店',
    section: 'navigate',
    run: () => useWorkbenchStore.getState().openTab('store'),
  },
  {
    id: 'tab.skills',
    label: '打开 Skill 管理',
    section: 'navigate',
    run: () => useWorkbenchStore.getState().openTab('skills'),
  },
  {
    id: 'bottom.toggle',
    label: '切换底部面板',
    section: 'view',
    shortcut: 'Ctrl+J',
    run: () => useWorkbenchStore.getState().toggleBottom(),
  },
  {
    id: 'settings.open',
    label: '打开设置',
    section: 'view',
    run: () => useOverlayStore.getState().open('settings'),
  },
  {
    id: 'pane.sessions',
    label: '切换会话栏',
    section: 'view',
    run: () => useWorkbenchStore.getState().togglePane('sessions'),
  },
  {
    id: 'pane.chat',
    label: '切换对话栏',
    section: 'view',
    run: () => useWorkbenchStore.getState().togglePane('chat'),
  },
  {
    id: 'pane.inspector',
    label: '切换 Inspector',
    section: 'view',
    run: () => useWorkbenchStore.getState().togglePane('inspector'),
  },
  {
    id: 'pane.chatMini',
    label: '缩小/还原对话窗口',
    section: 'view',
    run: () => useWorkbenchStore.getState().toggleChatMini(),
  },
  {
    id: 'theme.toggle',
    label: '切换主题（浅色/深色）',
    section: 'view',
    run: () => useThemeStore.getState().toggleMode(),
  },
  {
    id: 'palette.open',
    label: '命令面板',
    section: 'view',
    shortcut: 'Ctrl+K',
    run: () => useOverlayStore.getState().open('palette'),
  },
  {
    id: 'help.shortcuts',
    label: '键盘快捷键',
    section: 'view',
    run: () => useOverlayStore.getState().open('shortcuts'),
  },
  {
    id: 'help.about',
    label: '关于 RurixForge',
    section: 'view',
    run: () => useOverlayStore.getState().open('about'),
  },
];

export function runCommand(id: string): void {
  COMMANDS.find((c) => c.id === id)?.run();
}

/** 参考 palette_commands 过滤:label/id 子串(小写)命中 */
export function filterCommands(query: string): Command[] {
  const q = query.trim().toLowerCase();
  if (q === '') return COMMANDS;
  return COMMANDS.filter((c) => c.label.toLowerCase().includes(q) || c.id.includes(q));
}

export const SECTION_LABELS: Record<CommandSection, string> = {
  agent: 'Agent',
  navigate: '导航',
  view: '视图',
};
