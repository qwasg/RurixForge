import { Bug, Infinity as InfinityIcon, ListTree, MessageSquareText, Palette, Split, Users, Wand2, type LucideIcon } from 'lucide-react';

/**
 * composer 七模式(参考 COMPOSER_MODE_OPTIONS:Build 显示为 Agent)。
 * lucide-react 已在依赖内(wave.3 壳同库),图标:Agent=infinity / Plan=list-tree /
 * UltraPlan=wand-2 / Team=users / Debug=bug / Multitask=split / Ask=message-square-text。
 * Team = 游戏制作特化多代理:leader 统筹立项→素材→场景→逻辑→测试,task 派专职子代理。
 * UltraPlan(D-044)= 从一句游戏设想到 MVP 的引导式流程:深度规划 → 问卷 → 网页 Demo →
 * 制作计划 → Team 一次产出;本地与 Codex 引擎的 coding 代理可用。
 * Design(D-045)= 设计意图 → 生图出设计稿 → 用户审阅(挑选 / 修改 / 重出)→ 引擎内原子级复刻;
 * 本地与 Codex 引擎的 coding 代理可用(需要能看图的模型)。
 */
export interface ComposerModeMeta {
  id: string;
  label: string;
  icon: LucideIcon;
  iconClassName: string;
}

export const COMPOSER_MODES: ComposerModeMeta[] = [
  { id: 'build', label: 'Agent', icon: InfinityIcon, iconClassName: 'text-[color:var(--mode-agent-icon)]' },
  { id: 'plan', label: 'Plan', icon: ListTree, iconClassName: 'text-[color:var(--mode-plan-icon)]' },
  { id: 'ultraplan', label: 'UltraPlan', icon: Wand2, iconClassName: 'text-[color:var(--mode-ultraplan-icon)]' },
  { id: 'design', label: 'Design', icon: Palette, iconClassName: 'text-[color:var(--mode-design-icon)]' },
  { id: 'team', label: 'Team', icon: Users, iconClassName: 'text-[color:var(--mode-team-icon)]' },
  { id: 'debug', label: 'Debug', icon: Bug, iconClassName: 'text-[color:var(--mode-debug-icon)]' },
  { id: 'multitask', label: 'Multitask', icon: Split, iconClassName: 'text-[color:var(--mode-multitask-icon)]' },
  { id: 'ask', label: 'Ask', icon: MessageSquareText, iconClassName: 'text-[color:var(--mode-ask-icon)]' },
];

export function composerModeMeta(id: string): ComposerModeMeta {
  return COMPOSER_MODES.find((m) => m.id === id) ?? COMPOSER_MODES[0];
}

/** 按代理类型与引擎过滤可选模式(Composer 与消息卡回退编辑共用)。 */
export function modesForKind(kind: string, engine: 'local' | 'codex' = 'local') {
  let modes = COMPOSER_MODES;
  if (kind === 'general' || kind === 'document') {
    modes = COMPOSER_MODES.filter((m) => m.id === 'ask' || m.id === 'build');
  }
  // Codex 支持 UltraPlan / Team;后台 Multitask 尚无对应运行语义。
  return engine === 'codex'
    ? modes.filter((m) => m.id !== 'multitask')
    : modes;
}
