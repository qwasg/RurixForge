import { Bug, Infinity as InfinityIcon, ListTree, MessageSquareText, Split, type LucideIcon } from 'lucide-react';

/**
 * F7 wave.4 composer 五模式(参考 COMPOSER_MODE_OPTIONS:Build 显示为 Agent)。
 * lucide-react 已在依赖内(wave.3 壳同库),图标:Agent=infinity / Plan=list-tree /
 * Debug=bug / Multitask=split / Ask=message-square-text。
 */
export interface ComposerModeMeta {
  id: string;
  label: string;
  icon: LucideIcon;
}

export const COMPOSER_MODES: ComposerModeMeta[] = [
  { id: 'build', label: 'Agent', icon: InfinityIcon },
  { id: 'plan', label: 'Plan', icon: ListTree },
  { id: 'debug', label: 'Debug', icon: Bug },
  { id: 'multitask', label: 'Multitask', icon: Split },
  { id: 'ask', label: 'Ask', icon: MessageSquareText },
];

export function composerModeMeta(id: string): ComposerModeMeta {
  return COMPOSER_MODES.find((m) => m.id === id) ?? COMPOSER_MODES[0];
}
