/**
 * 设置页 tab 单一事实源(07 §7.1:cindy tabLabels.ts 模式逐字对齐)。
 * 首发冻结清单见 07 §7.2;F3 仅 skills tab 真实落地,其余 tab 占位如实标注承接里程碑。
 */

export const TAB_IDS = [
  'general',
  'providers',
  'subagent-models',
  'mcp',
  'skills',
  'permissions',
  'viewport',
  'generation',
  'shortcuts',
  'storage',
  'about',
] as const;

export type SettingsTab = (typeof TAB_IDS)[number];

export const TAB_LABELS: Record<SettingsTab, string> = {
  general: 'General',
  providers: 'Providers',
  'subagent-models': 'Subagent Models',
  mcp: 'MCP',
  skills: 'Skills',
  permissions: 'Permissions',
  viewport: 'Viewport',
  generation: 'Generation',
  shortcuts: 'Shortcuts',
  storage: 'Storage',
  about: 'About',
};

/** F3 真实落地的 tab;其余占位(标注承接里程碑,不伪造功能)。 */
export const TAB_Landed: Record<SettingsTab, string | null> = {
  general: null,
  providers: null,
  'subagent-models': null,
  mcp: null,
  skills: 'F3',
  permissions: null,
  viewport: null,
  generation: 'F5',
  shortcuts: null,
  storage: null,
  about: null,
};

export const DEFAULT_TAB: SettingsTab = 'skills';

export function isSettingsTab(v: unknown): v is SettingsTab {
  return typeof v === 'string' && (TAB_IDS as readonly string[]).includes(v);
}
