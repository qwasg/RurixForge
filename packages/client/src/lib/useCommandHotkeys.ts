import { useEffect, useLayoutEffect, useRef } from 'react';

export const COMMAND_HOTKEYS = [
  ['左键 / 拖框', '选择单位；Shift 加选'], ['右键 / A 后左键', '移动所选 AI 或移动基站'],
  ['中键拖动 / 方向键', '平移地图'], ['滚轮 / + −', '缩放地图'], ['Home', '回到全图'],
  ['B / U / I', '建筑 / 单位 / 显卡牌组'], ['1–9', '选择当前牌组卡片'], ['V', '收起或展开牌组'],
  ['L / C', '电力线 / 算力线：依次点击两个端点'], ['W', '拖动修建 cudad 护城河'],
  ['F', '为闭合城墙内的拓扑区域充盾'], ['Q', '所选角色技能瞄准'], ['E / R', '升级 / 回收所选对象'],
  ['T', '切换所选单位目标策略'], ['Enter', '在所选数据中心安装当前显卡'],
  ['G', '切换地形与网络覆盖'], ['N', '发起下一波入侵'], ['Space', '暂停或继续'],
  ['Esc', '取消当前工具或关闭浮层'], ['H', '打开指挥手册'],
] as const;

export type CommandHotkeyOptions = {
  enabled: boolean; blocked: boolean;
  selectCard(index: number): void;
  deck(kind: 'buildings' | 'units' | 'gpus'): void;
  toggleDeck(): void;
  tool(kind: 'power' | 'compute' | 'wall' | 'shield' | 'move' | 'skill'): void;
  upgrade(): void; recycle(): void; target(): void; install(): void;
  overlay(): void; nextWave(): void; pause(): void; cancel(): void; help(): void;
  pan(x: number, y: number): void; zoom(delta: number): void; home(): void;
};

function editing(event: KeyboardEvent): boolean {
  return (event.composedPath?.() ?? [event.target]).some(target => target instanceof Element && (
    target.closest('input,textarea,select,[role="textbox"],[role="combobox"]') !== null
    || (target as HTMLElement).isContentEditable
    || Boolean(target.closest('[contenteditable]:not([contenteditable="false"])'))
  ));
}

/** Shortcuts issue intentions only. Purchases, movement and combat remain native-authoritative. */
export function useCommandHotkeys(options: CommandHotkeyOptions): void {
  const current = useRef(options);
  useLayoutEffect(() => { current.current = options; }, [options]);
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      const action = current.current;
      if (!action.enabled || event.defaultPrevented || event.isComposing || event.ctrlKey
        || event.metaKey || event.altKey || editing(event)) return;
      const key = event.key.toLowerCase();
      if (event.repeat && !key.startsWith('arrow')) return;
      if ((key === ' ' || key === 'enter') && event.target instanceof Element
        && event.target.closest('button,a[href],summary,[role="button"],[role="tab"]')) return;
      const run = (callback: () => void) => { event.preventDefault(); callback(); };
      if (action.blocked) { if (key === 'escape') run(action.cancel); return; }
      if (/^[1-9]$/.test(key)) { run(() => action.selectCard(Number(key) - 1)); return; }
      switch (key) {
        case 'b': run(() => action.deck('buildings')); break;
        case 'u': run(() => action.deck('units')); break;
        case 'i': run(() => action.deck('gpus')); break;
        case 'v': run(action.toggleDeck); break;
        case 'l': run(() => action.tool('power')); break;
        case 'c': run(() => action.tool('compute')); break;
        case 'w': run(() => action.tool('wall')); break;
        case 'f': run(() => action.tool('shield')); break;
        case 'a': run(() => action.tool('move')); break;
        case 'q': run(() => action.tool('skill')); break;
        case 'e': run(action.upgrade); break;
        case 'r': run(action.recycle); break;
        case 't': run(action.target); break;
        case 'enter': run(action.install); break;
        case 'g': run(action.overlay); break;
        case 'n': run(action.nextWave); break;
        case ' ': run(action.pause); break;
        case 'escape': run(action.cancel); break;
        case 'h': run(action.help); break;
        case 'home': run(action.home); break;
        case '+': case '=': run(() => action.zoom(.2)); break;
        case '-': case '_': run(() => action.zoom(-.2)); break;
        case 'arrowleft': run(() => action.pan(60, 0)); break;
        case 'arrowright': run(() => action.pan(-60, 0)); break;
        case 'arrowup': run(() => action.pan(0, 60)); break;
        case 'arrowdown': run(() => action.pan(0, -60)); break;
      }
    };
    window.addEventListener('keydown', listener);
    return () => window.removeEventListener('keydown', listener);
  }, []);
}
