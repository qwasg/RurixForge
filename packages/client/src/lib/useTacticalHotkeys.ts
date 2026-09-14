import { useEffect, useLayoutEffect, useRef } from 'react';

export type TacticalHandMode = 'operators' | 'hardware';

/** Callbacks own selection validation and native command dispatch; this hook changes no game state. */
export type TacticalHotkeyOptions = {
  enabled: boolean;
  blocked: boolean;
  mode: TacticalHandMode;
  /** True only while a deployed unit or occupied GPU socket is selected. */
  hasSelection: boolean;
  selectCard(index: number): void;
  toggleHand(): void;
  toggleDeck?(): void;
  activateSkill(): void;
  upgrade(): void;
  sell(): void;
  cycleTarget(): void;
  nextWave(): void;
  togglePause(): void;
  cancel(): void;
  cycleUnit(delta: number): void;
  quickInstall(): void;
  help(): void;
  toggleGrid(): void;
  toggleSound(): void;
};

export type TacticalHotkeyInfo = {
  id: string;
  action: keyof Omit<TacticalHotkeyOptions, 'enabled' | 'blocked' | 'mode' | 'hasSelection'>;
  keys: readonly string[];
  keyLabel: string;
  label: string;
  description: string;
  mode?: TacticalHandMode;
  requiresSelection?: boolean;
  optional?: boolean;
};

/** The actual binding catalogue for the in-game help. Tab deliberately remains browser focus navigation. */
export const TACTICAL_HOTKEYS: readonly TacticalHotkeyInfo[] = [
  { id: 'operator-card', action: 'selectCard', keys: ['1', '2', '3', '4'], keyLabel: '1–4', mode: 'operators', label: '选择角色卡', description: '按卡牌显示顺序选择角色。' },
  { id: 'hardware-card', action: 'selectCard', keys: ['1', '2', '3', '4', '5', '6', '7'], keyLabel: '1–7', mode: 'hardware', label: '选择显卡卡牌', description: '选择显卡牌组中的对应型号。' },
  { id: 'hand-mode', action: 'toggleHand', keys: ['B'], keyLabel: 'B', label: '切换角色 / 显卡牌组', description: '切换当前卡牌类型。' },
  { id: 'deck-visibility', action: 'toggleDeck', keys: ['V'], keyLabel: 'V', optional: true, label: '收起 / 展开整个手牌', description: '展开完整手牌，或收起手牌以查看战场。' },
  { id: 'quick-install', action: 'quickInstall', keys: ['Enter'], keyLabel: 'Enter', mode: 'hardware', label: '快速安装显卡', description: '将所选显卡安装到首个空机架，需要足够建设经费。' },
  { id: 'skill', action: 'activateSkill', keys: ['Q'], keyLabel: 'Q', label: '选择技能落点', description: '选中已部署的守护者后，进入技能落点选择。' },
  { id: 'upgrade', action: 'upgrade', keys: ['E'], keyLabel: 'E', requiresSelection: true, label: '升级选中对象', description: '仅选中已部署单位或已安装显卡时处理。' },
  { id: 'sell', action: 'sell', keys: ['R'], keyLabel: 'R', requiresSelection: true, label: '回收选中对象', description: '回收已选中的守护者或显卡；仅选择待购买卡牌不会回收任何对象。' },
  { id: 'target', action: 'cycleTarget', keys: ['T'], keyLabel: 'T', requiresSelection: true, label: '切换目标策略', description: '在所选守护者的普攻目标策略间切换。' },
  { id: 'previous-unit', action: 'cycleUnit', keys: ['Z'], keyLabel: 'Z', label: '上一个已部署单位', description: '向前选择战场上的已部署守护者。' },
  { id: 'next-unit', action: 'cycleUnit', keys: ['X'], keyLabel: 'X', label: '下一个已部署单位', description: '向后选择战场上的已部署守护者。' },
  { id: 'wave', action: 'nextWave', keys: ['N'], keyLabel: 'N', label: '开启下一波', description: '准备结束后开启下一波入侵。' },
  { id: 'pause', action: 'togglePause', keys: ['Space'], keyLabel: 'Space', label: '暂停 / 继续', description: '暂停或继续当前战斗。' },
  { id: 'cancel', action: 'cancel', keys: ['Escape'], keyLabel: 'Esc', label: '取消 / 收起', description: '依次取消技能或部署、关闭详情，再收起手牌；弹窗内按Esc关闭弹窗。' },
  { id: 'help', action: 'help', keys: ['H'], keyLabel: 'H', label: '打开帮助', description: '查看操作说明和快捷键。' },
  { id: 'grid', action: 'toggleGrid', keys: ['G'], keyLabel: 'G', label: '显示 / 隐藏网格', description: '切换战场辅助网格。' },
  { id: 'sound', action: 'toggleSound', keys: ['M'], keyLabel: 'M', label: '开启 / 关闭音效', description: '切换游戏音效。' },
];

/** Optional V is listed only when that callback exists in the integrating UI. */
export function getTacticalHotkeys(mode: TacticalHandMode, options: { toggleDeck?: boolean } = {}) {
  return TACTICAL_HOTKEYS.filter((item) => (!item.mode || item.mode === mode)
    && (!item.optional || options.toggleDeck === true));
}

function isEditing(event: KeyboardEvent): boolean {
  const path = event.composedPath?.() ?? [event.target];
  return path.some((target) => {
    if (!(target instanceof Element)) return false;
    if (target.closest('input,textarea,select,[role="textbox"],[role="combobox"]')) return true;
    if ((target as HTMLElement).isContentEditable) return true;
    const editable = target.closest('[contenteditable]');
    return editable !== null && editable.getAttribute('contenteditable')?.toLowerCase() !== 'false';
  });
}

function hasNativeActivation(event: KeyboardEvent): boolean {
  return (event.composedPath?.() ?? [event.target]).some((target) => target instanceof Element
    && target.closest('button,a[href],summary,[role="button"],[role="link"],[role="menuitem"],[role="tab"]') !== null);
}

export function useTacticalHotkeys(options: TacticalHotkeyOptions): void {
  const current = useRef(options);
  // A stable listener always observes committed selection/mode/callbacks, including native updates.
  useLayoutEffect(() => { current.current = options; }, [options]);
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      const action = current.current;
      if (!action.enabled || event.defaultPrevented || event.repeat || event.isComposing
        || event.ctrlKey || event.metaKey || event.altKey || isEditing(event)) return;
      const key = event.key.toLowerCase();
      // A focused button's Enter/Space activates that button, not a different purchase or pause action.
      if ((key === 'enter' || key === ' ') && hasNativeActivation(event)) return;
      const invoke = (callback: (() => void) | undefined) => {
        if (!callback) return;
        event.preventDefault();
        callback();
      };
      if (action.blocked) {
        if (key === 'escape') invoke(action.cancel);
        return;
      }
      if (/^[1-7]$/.test(key)) {
        const index = Number(key) - 1;
        if (index < (action.mode === 'operators' ? 4 : 7)) invoke(() => action.selectCard(index));
        return;
      }
      switch (key) {
        case 'b': invoke(action.toggleHand); break;
        case 'v': invoke(action.toggleDeck); break;
        case 'enter': if (action.mode === 'hardware') invoke(action.quickInstall); break;
        case 'q': invoke(action.activateSkill); break;
        case 'e': if (action.hasSelection) invoke(action.upgrade); break;
        case 'r': if (action.hasSelection) invoke(action.sell); break;
        case 't': if (action.hasSelection) invoke(action.cycleTarget); break;
        case 'z': invoke(() => action.cycleUnit(-1)); break;
        case 'x': invoke(() => action.cycleUnit(1)); break;
        case 'n': invoke(action.nextWave); break;
        case ' ': invoke(action.togglePause); break;
        case 'escape': invoke(action.cancel); break;
        case 'h': invoke(action.help); break;
        case 'g': invoke(action.toggleGrid); break;
        case 'm': invoke(action.toggleSound); break;
      }
    };
    window.addEventListener('keydown', keydown);
    return () => window.removeEventListener('keydown', keydown);
  }, []);
}
