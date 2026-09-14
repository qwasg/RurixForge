import { Bug, Infinity as InfinityIcon, ListTree, MessageSquareText, Split, Users, type LucideIcon } from 'lucide-react';

/**
 * composer 六模式(参考 COMPOSER_MODE_OPTIONS:Build 显示为 Agent)。
 * lucide-react 已在依赖内(wave.3 壳同库),图标:Agent=infinity / Plan=list-tree /
 * Team=users / Debug=bug / Multitask=split / Ask=message-square-text。
 * Team = 游戏制作特化多代理:leader 统筹立项→素材→场景→逻辑→测试,task 派专职子代理。
 */
export interface ComposerModeMeta {
  id: string;
  label: string;
  icon: LucideIcon;
}

export const COMPOSER_MODES: ComposerModeMeta[] = [
  { id: 'build', label: 'Agent', icon: InfinityIcon },
  { id: 'plan', label: 'Plan', icon: ListTree },
  { id: 'team', label: 'Team', icon: Users },
  { id: 'debug', label: 'Debug', icon: Bug },
  { id: 'multitask', label: 'Multitask', icon: Split },
  { id: 'ask', label: 'Ask', icon: MessageSquareText },
];

export function composerModeMeta(id: string): ComposerModeMeta {
  return COMPOSER_MODES.find((m) => m.id === id) ?? COMPOSER_MODES[0];
}
