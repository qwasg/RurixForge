import { cleanup, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { getTacticalHotkeys, TACTICAL_HOTKEYS, useTacticalHotkeys, type TacticalHotkeyOptions } from '@/lib/useTacticalHotkeys';

function options(overrides: Partial<TacticalHotkeyOptions> = {}): TacticalHotkeyOptions {
  return { enabled: true, blocked: false, mode: 'operators', hasSelection: false,
    selectCard: vi.fn(), toggleHand: vi.fn(), toggleDeck: vi.fn(), activateSkill: vi.fn(),
    upgrade: vi.fn(), sell: vi.fn(), cycleTarget: vi.fn(), nextWave: vi.fn(), togglePause: vi.fn(),
    cancel: vi.fn(), cycleUnit: vi.fn(), quickInstall: vi.fn(), help: vi.fn(), toggleGrid: vi.fn(),
    toggleSound: vi.fn(), ...overrides };
}
function press(key: string, extra: KeyboardEventInit = {}, target: EventTarget = document.body) {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra });
  target.dispatchEvent(event);
  return event;
}
afterEach(() => { cleanup(); document.body.replaceChildren(); vi.restoreAllMocks(); });

describe('tactical keyboard callbacks', () => {
  it('maps only valid cards to zero-based indices and uses the latest committed hand mode', () => {
    const first = options();
    const { rerender } = renderHook((value) => useTacticalHotkeys(value), { initialProps: first });
    expect(press('1').defaultPrevented).toBe(true);
    press('4');
    expect(press('5').defaultPrevented).toBe(false);
    expect(first.selectCard).toHaveBeenCalledTimes(2);
    expect(first.selectCard).toHaveBeenNthCalledWith(1, 0);
    expect(first.selectCard).toHaveBeenNthCalledWith(2, 3);
    const hardware = options({ mode: 'hardware' });
    rerender(hardware);
    press('1'); press('7');
    expect(press('8').defaultPrevented).toBe(false);
    expect(hardware.selectCard).toHaveBeenNthCalledWith(1, 0);
    expect(hardware.selectCard).toHaveBeenNthCalledWith(2, 6);
    expect(first.selectCard).toHaveBeenCalledTimes(2);
  });

  it('distinguishes B hand type from V whole-deck visibility and leaves Tab untouched', () => {
    const calls = options();
    renderHook(() => useTacticalHotkeys(calls));
    expect(press('B').defaultPrevented).toBe(true);
    expect(press('v').defaultPrevented).toBe(true);
    expect(calls.toggleHand).toHaveBeenCalledOnce();
    expect(calls.toggleDeck).toHaveBeenCalledOnce();
    const button = document.createElement('button'); document.body.append(button); button.focus();
    for (const target of [document.body, button]) {
      expect(press('Tab', {}, target).defaultPrevented).toBe(false);
      expect(press('Tab', { shiftKey: true }, target).defaultPrevented).toBe(false);
    }
    expect(calls.toggleHand).toHaveBeenCalledOnce();
    expect(calls.toggleDeck).toHaveBeenCalledOnce();
  });

  it('quick-installs only in hardware mode and keeps an unsupported optional V unclaimed', () => {
    const calls = options({ toggleDeck: undefined });
    const { rerender } = renderHook((value) => useTacticalHotkeys(value), { initialProps: calls });
    expect(press('Enter').defaultPrevented).toBe(false);
    expect(press('v').defaultPrevented).toBe(false);
    expect(calls.quickInstall).not.toHaveBeenCalled();
    rerender({ ...calls, mode: 'hardware' });
    expect(press('Enter').defaultPrevented).toBe(true);
    expect(calls.quickInstall).toHaveBeenCalledOnce();
  });

  it('requires actual selection for E/R/T and immediately stops destructive shortcuts after selection is lost', () => {
    const calls = options();
    const { rerender } = renderHook((value) => useTacticalHotkeys(value), { initialProps: calls });
    for (const key of ['e', 'r', 't']) expect(press(key).defaultPrevented).toBe(false);
    expect(calls.sell).not.toHaveBeenCalled();
    rerender({ ...calls, hasSelection: true });
    for (const key of ['E', 'R', 'T']) expect(press(key).defaultPrevented).toBe(true);
    expect(calls.upgrade).toHaveBeenCalledOnce();
    expect(calls.sell).toHaveBeenCalledOnce();
    expect(calls.cycleTarget).toHaveBeenCalledOnce();
    rerender({ ...calls, hasSelection: false });
    expect(press('r').defaultPrevented).toBe(false);
    expect(calls.sell).toHaveBeenCalledOnce();
  });

  it('preserves Enter/Space activation of focused controls instead of buying another card or pausing', () => {
    const calls = options({ mode: 'hardware' }); renderHook(() => useTacticalHotkeys(calls));
    const button = document.createElement('button'), link = document.createElement('a'), nested = document.createElement('span');
    link.href = '/help'; button.append(nested); document.body.append(button, link);
    for (const target of [button, nested, link]) {
      expect(press('Enter', {}, target).defaultPrevented).toBe(false);
      expect(press(' ', {}, target).defaultPrevented).toBe(false);
    }
    expect(calls.quickInstall).not.toHaveBeenCalled(); expect(calls.togglePause).not.toHaveBeenCalled();
    expect(press('Enter').defaultPrevented).toBe(true);
    expect(calls.quickInstall).toHaveBeenCalledOnce();
    expect(press(' ').defaultPrevented).toBe(true);
    expect(calls.togglePause).toHaveBeenCalledOnce();
  });

  it('dispatches the remaining actual tactical actions once and passes unit-cycle directions', () => {
    const calls = options(); renderHook(() => useTacticalHotkeys(calls));
    for (const key of ['q', 'n', ' ', 'Escape', 'h', 'g', 'm', 'z', 'x']) expect(press(key).defaultPrevented).toBe(true);
    for (const action of ['activateSkill', 'nextWave', 'togglePause', 'cancel', 'help', 'toggleGrid', 'toggleSound'] as const) {
      expect(calls[action]).toHaveBeenCalledOnce();
    }
    expect(calls.cycleUnit).toHaveBeenNthCalledWith(1, -1);
    expect(calls.cycleUnit).toHaveBeenNthCalledWith(2, 1);
    expect(calls.selectCard).not.toHaveBeenCalled();
  });

  it('blocks all gameplay through a modal while allowing only its Escape cancellation', () => {
    const calls = options({ blocked: true, hasSelection: true, mode: 'hardware' });
    renderHook(() => useTacticalHotkeys(calls));
    for (const key of ['1', '7', 'b', 'v', 'Enter', 'q', 'e', 'r', 't', 'z', 'x', 'n', ' ', 'h', 'g', 'm']) {
      expect(press(key).defaultPrevented).toBe(false);
    }
    for (const value of Object.values(calls)) if (vi.isMockFunction(value)) expect(value).not.toHaveBeenCalled();
    expect(press('Escape').defaultPrevented).toBe(true);
    expect(calls.cancel).toHaveBeenCalledOnce();
  });

  it('never steals text-editor, textarea or select input, including nested and shadow-DOM editables', () => {
    const calls = options({ hasSelection: true, mode: 'hardware' }); renderHook(() => useTacticalHotkeys(calls));
    const input = document.createElement('input'), textarea = document.createElement('textarea'), select = document.createElement('select');
    const editor = document.createElement('div'), text = document.createElement('span'); editor.setAttribute('contenteditable', 'true'); editor.append(text);
    const plaintext = document.createElement('div'); plaintext.setAttribute('contenteditable', 'plaintext-only');
    const roleTextbox = document.createElement('div'); roleTextbox.setAttribute('role', 'textbox');
    document.body.append(input, textarea, select, editor, plaintext, roleTextbox);
    for (const target of [input, textarea, select, text, plaintext, roleTextbox]) {
      for (const key of ['1', 'q', 'r', 'Enter', 'Escape', ' ']) expect(press(key, {}, target).defaultPrevented).toBe(false);
    }
    const host = document.createElement('div'); document.body.append(host);
    const shadow = host.attachShadow({ mode: 'open' }), shadowInput = document.createElement('input'); shadow.append(shadowInput);
    expect(press('r', { composed: true }, shadowInput).defaultPrevented).toBe(false);
    expect(calls.sell).not.toHaveBeenCalled();
    expect(calls.cancel).not.toHaveBeenCalled();
    expect(calls.quickInstall).not.toHaveBeenCalled();
  });

  it('ignores modifiers, IME composition, held-key repeats, already-consumed events and disabled state', () => {
    const calls = options({ hasSelection: true });
    const { rerender } = renderHook((value) => useTacticalHotkeys(value), { initialProps: calls });
    for (const extra of [{ ctrlKey: true }, { metaKey: true }, { altKey: true }, { repeat: true }, { isComposing: true }]) {
      expect(press('r', extra).defaultPrevented).toBe(false);
    }
    const handled = new KeyboardEvent('keydown', { key: 'r', bubbles: true, cancelable: true });
    handled.preventDefault(); document.body.dispatchEvent(handled);
    expect(calls.sell).not.toHaveBeenCalled();
    rerender({ ...calls, enabled: false });
    for (const key of ['r', 'q', '1', 'Escape']) expect(press(key).defaultPrevented).toBe(false);
    expect(calls.activateSkill).not.toHaveBeenCalled();
    expect(calls.cancel).not.toHaveBeenCalled();
  });

  it('prevents defaults before invoking callbacks and unregisters the listener on unmount', () => {
    let event!: KeyboardEvent;
    const calls = options({ sell: vi.fn(() => expect(event.defaultPrevented).toBe(true)), hasSelection: true });
    const { unmount } = renderHook(() => useTacticalHotkeys(calls));
    event = new KeyboardEvent('keydown', { key: 'r', bubbles: true, cancelable: true });
    document.body.dispatchEvent(event);
    expect(calls.sell).toHaveBeenCalledOnce();
    unmount();
    expect(press('r').defaultPrevented).toBe(false);
    expect(calls.sell).toHaveBeenCalledOnce();
  });
});

describe('tactical hotkey help metadata', () => {
  it('matches mode-specific card/install keys and explicitly separates optional V from B', () => {
    const operators = getTacticalHotkeys('operators');
    expect(operators.find((item) => item.action === 'selectCard')?.keys).toEqual(['1', '2', '3', '4']);
    expect(operators.some((item) => item.action === 'quickInstall')).toBe(false);
    expect(operators.some((item) => item.action === 'toggleDeck')).toBe(false);
    const hardware = getTacticalHotkeys('hardware', { toggleDeck: true });
    expect(hardware.find((item) => item.action === 'selectCard')?.keys).toHaveLength(7);
    expect(hardware.find((item) => item.action === 'quickInstall')?.keys).toEqual(['Enter']);
    expect(hardware.find((item) => item.action === 'toggleDeck')).toMatchObject({ keyLabel: 'V', label: '收起 / 展开整个手牌', optional: true });
    expect(hardware.find((item) => item.action === 'toggleHand')).toMatchObject({ keyLabel: 'B', label: '切换角色 / 显卡牌组' });
    expect(TACTICAL_HOTKEYS.some((item) => item.keys.includes('Tab'))).toBe(false);
    expect(TACTICAL_HOTKEYS.find((item) => item.action === 'sell')?.requiresSelection).toBe(true);
  });
});
