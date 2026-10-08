import { act, cleanup, createEvent, fireEvent, render, renderHook, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import CodeSentinelsCards from '@/views/CodeSentinelsCards';
import { useSentinelsRuntime } from '@/lib/useSentinelsRuntime';
import { TACTICAL_CARD_MIME } from '@/components/game/TacticalCard';
import type { ViewportStreamOptions } from '@/lib/viewportStream';
import captured from './fixtures/sentinels-v2-native.json';

type Entity = { name: string; transform: { translation: number[]; scale: number[] } };
type Publication = { entities: Entity[] };
type Stream = { options: ViewportStreamOptions; up: boolean; close: ReturnType<typeof vi.fn> };
const native = vi.hoisted(() => ({ entities: [] as Entity[], reads: [] as Promise<Publication>[], streams: [] as Stream[],
  callTool: vi.fn(), apiGet: vi.fn(), sendInput: vi.fn(() => true), setActive: vi.fn(), loadFailure: null as Error | null }));
vi.mock('@/lib/forgeApi', () => ({ callTool: native.callTool, apiGet: native.apiGet }));
vi.mock('@/lib/workspaceStore', () => ({ useWorkspaceStore: { getState: () => ({ setActive: native.setActive }) } }));
vi.mock('@/lib/viewportStream', () => ({ openViewportStream: (options: ViewportStreamOptions) => {
  const stream: Stream = { options, up: false, close: vi.fn() };
  stream.close.mockImplementation(() => { stream.up = false; options.onChannel?.(false, 'closed'); });
  native.streams.push(stream);
  return { get up() { return stream.up; }, sendInput: native.sendInput, close: stream.close,
    sendPointer: vi.fn(), sendCamera: vi.fn(), setSelected: vi.fn(), resize: vi.fn() };
} }));
const clone = (): Entity[] => structuredClone(captured.entities);
function set(entities: Entity[], name: string, fields: number[]) {
  entities.find((entity) => entity.name === name)!.transform = { translation: fields.slice(0, 3), scale: fields.slice(3, 6) };
}
function clearUnit(entities: Entity[], slot: number) {
  set(entities, `CS_Unit${slot}`, [0, 0, -1, 0, 0, 0]);
  set(entities, `CS_UnitAux${slot}`, [0, 0, 0, 0, 0, 0]);
  set(entities, `CS_UnitCost${slot}`, [0, 0, 0, 0, 0, 0]);
}
/** Native publications derived from the captured DLL schema, never a browser combat simulation. */
function initial() {
  const entities = clone(); clearUnit(entities, 0);
  set(entities, 'CS_State', [0, 20, 0, 0, 0, 1]);
  set(entities, 'CS_Economy', [560, 0, 0, 0, 0, 0]);
  set(entities, 'CS_GPU0', [0, 0, 0, 0, 0, 0]);
  set(entities, 'CS_Meta', [0, 0, 0, 0, 0, 0]);
  return entities;
}
function installed() {
  const entities = clone(); clearUnit(entities, 0);
  set(entities, 'CS_Economy', [430, 12, 300, 0, 0, 1]);
  set(entities, 'CS_Meta', [20, 130, 2, 0, 0, 0]);
  return entities;
}
function deployed() {
  const entities = installed();
  set(entities, 'CS_State', [200, 20, 0, 0, 0, 1]);
  set(entities, 'CS_Economy', [320, 12, 300, 0, 0, 1]);
  set(entities, 'CS_Unit6', [3, 2, 159, 140, 0, 97]);
  set(entities, 'CS_UnitAux6', [7.25, 6.75, 0, 14, 0, 0]);
  set(entities, 'CS_UnitCost6', [116, 182, 260, 2, 0, 0]);
  set(entities, 'CS_Meta', [10, 110, 3, 0, 0, 0]);
  return entities;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
const key = (value: string, extra: KeyboardEventInit = {}) => fireEvent.keyDown(document.body, { key: value, ...extra });
const hand = () => screen.getByRole('button', { name: '切换角色与显卡牌组' });
function chooseHand(mode: 'operators' | 'hardware') {
  if (!hand().textContent?.includes(mode.toUpperCase())) key('b');
  expect(hand()).toHaveTextContent(mode.toUpperCase());
}
async function connect(stream = native.streams.at(-1)!) {
  await act(async () => { stream.up = true; stream.options.onChannel?.(true, 'connected'); });
}
async function enter() {
  render(<CodeSentinelsCards/>);
  await act(async () => {});
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: /开始行动/ })); });
  await connect();
}
async function publish() { await act(async () => { await vi.advanceTimersByTimeAsync(400); }); }
function drop(payload: unknown, cell: number, target?: HTMLElement) {
  const board = screen.getByTestId('v3-board');
  vi.spyOn(board, 'getBoundingClientRect').mockReturnValue({ left: 100, top: 80, width: 1280, height: 720,
    x: 100, y: 80, right: 1380, bottom: 800, toJSON: () => ({}) });
  const event = createEvent.drop(target ?? board, { dataTransfer: { types: [TACTICAL_CARD_MIME], getData: (type: string) => type === TACTICAL_CARD_MIME ? JSON.stringify(payload) : '' } });
  Object.defineProperties(event, {
    clientX: { value: 100 + ((cell % 24 - 11.5) / (224 / 9) + .5) * 1280 },
    clientY: { value: 80 + (Math.floor(cell / 24) + .5) / 14 * 720 },
  });
  fireEvent(target ?? board, event);
}

beforeEach(() => {
  vi.useFakeTimers(); vi.stubGlobal('requestAnimationFrame', vi.fn(() => 1)); vi.stubGlobal('cancelAnimationFrame', vi.fn());
  history.replaceState({}, '', '/?play=code-sentinels&standalone=1');
  native.entities = initial(); native.reads = []; native.streams = []; native.loadFailure = null;
  native.sendInput.mockReset().mockReturnValue(true); native.setActive.mockReset();
  native.apiGet.mockReset().mockResolvedValue({ workspaces: [{ id: 'cards-fixture', name: 'Code Sentinels', root: 'D:/RurixForge/projects/code-sentinels' }] });
  native.callTool.mockReset().mockImplementation(async (name: string) => {
    if (name === 'scene_summary') return { playState: 'edit' };
    if (name === 'entity_list') return native.reads.shift() ?? { entities: structuredClone(native.entities) };
    if (name === 'scene_load' && native.loadFailure) { const error = native.loadFailure; native.loadFailure = null; throw error; }
    return {};
  });
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe('card edition with real runtime and native publication fixtures', () => {
  it('B/1/Enter sends GPU 4000001 once and switches to operators only after the native installation', async () => {
    await enter(); chooseHand('operators');
    key('b'); key('1'); key('Enter'); key('Enter');
    expect(native.sendInput.mock.calls).toEqual([['cs', 4_000_001]]);
    expect(hand()).toHaveTextContent('HARDWARE');
    expect(screen.getByTestId('v3-credits')).toHaveTextContent('560');
    expect(screen.getByTestId('v3-production')).toHaveTextContent('+0/s');
    native.entities = installed(); await publish();
    expect(hand()).toHaveTextContent('OPERATORS');
    expect(screen.getByTestId('v3-credits')).toHaveTextContent('430');
    expect(screen.getByTestId('v3-production')).toHaveTextContent('+12/s');
    expect(screen.getByRole('button', { name: '收起手牌' })).toBeInTheDocument();
  });

  it('preserves a pending GPU install across an old feedback publication until its own state arrives', async () => {
    set(native.entities, 'CS_Meta', [30, 0, 1, 0, 0, 0]);
    await enter();
    const old = deferred<Publication>(); native.reads.push(old.promise); await publish();
    const beforeCommand = structuredClone(native.entities);
    key('1'); key('Enter');
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 4_000_001);
    // A response already in flight can still carry the pre-command "install a GPU first" feedback.
    await act(async () => { old.resolve({ entities: beforeCommand }); });
    expect(hand()).toHaveTextContent('HARDWARE');
    native.entities = installed(); await publish();
    expect(hand()).toHaveTextContent('OPERATORS');
  });

  it('never automatically resends after disconnect but permits a manual retry once fresh state returns', async () => {
    await enter(); key('1'); key('Enter');
    expect(native.sendInput.mock.calls).toEqual([['cs', 4_000_001]]);
    const stream = native.streams[0];
    await act(async () => { stream.up = false; stream.options.onChannel?.(false, 'connection lost before acknowledgement'); });
    await publish();
    await connect(stream); await publish();
    // The native publication still has an empty socket: no successful purchase has been observed.
    expect(native.sendInput).toHaveBeenCalledTimes(1);
    key('Enter');
    expect(native.sendInput.mock.calls).toEqual([['cs', 4_000_001], ['cs', 4_000_001]]);
    expect(screen.getByTestId('v3-credits')).toHaveTextContent('560');
  });

  it('selects a card, collapses the hand and deploys the exact kind/cell without predicting money', async () => {
    native.entities = installed(); await enter(); chooseHand('operators');
    key('1');
    expect(document.querySelector('.cards-hand')).toHaveClass('is-collapsed');
    fireEvent.click(screen.getByTestId('v3-cell-159'));
    expect(native.sendInput.mock.calls).toEqual([['cs', 1_001_159]]);
    expect(screen.getByTestId('v3-credits')).toHaveTextContent('430');
    native.entities = clone(); await publish();
    expect(screen.getByTestId('v3-credits')).toHaveTextContent('365');
    expect(document.querySelector('.cards-hand')).toHaveClass('is-open');
  });

  it('does not discard pending deployment because the previous GPU-success feedback is replayed', async () => {
    native.entities = installed(); await enter(); chooseHand('operators');
    const old = deferred<Publication>(); native.reads.push(old.promise); await publish();
    const beforeCommand = structuredClone(native.entities);
    key('1'); fireEvent.click(screen.getByTestId('v3-cell-159'));
    await act(async () => { old.resolve({ entities: beforeCommand }); });
    native.entities = clone(); await publish();
    expect(document.querySelector('.cards-hand')).toHaveClass('is-open');
    native.sendInput.mockClear();
    fireEvent.click(screen.getByTestId('v3-cell-183'));
    expect(native.sendInput).not.toHaveBeenCalled();
  });

  it('Q uses the selected native slot and clicked target once; disappearing units never turn that target into deployment', async () => {
    native.entities = deployed(); await enter();
    fireEvent.click(screen.getByTestId('v3-cell-159')); key('q'); key('q', { repeat: true });
    expect(document.querySelector('.cards-aim-banner')).toHaveTextContent('97 CE');
    fireEvent.click(screen.getByTestId('v3-cell-183'));
    expect(native.sendInput.mock.calls).toEqual([['cs', 3_006_183]]);
    expect(screen.getByTestId('v3-energy')).toHaveTextContent('200');
    expect(document.querySelector('.cards-aim-banner')).toBeNull();
    key('q'); native.sendInput.mockClear(); clearUnit(native.entities, 6); await publish();
    expect(document.querySelector('.cards-aim-banner')).toBeNull();
    fireEvent.click(screen.getByTestId('v3-cell-184'));
    expect(native.sendInput).not.toHaveBeenCalled();
  });

  it('E/R act only on an existing selected native object, not a merely selected purchase card', async () => {
    native.entities = deployed(); await enter(); chooseHand('operators');
    key('3'); key('e'); key('r'); expect(native.sendInput).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId('v3-cell-159'));
    key('e'); key('r'); key('r');
    expect(native.sendInput.mock.calls).toEqual([['cs', 2_000_006], ['cs', 2_100_006]]);
    expect(screen.getByTestId('v3-credits')).toHaveTextContent('320');
  });

  it('Z starts from the last native unit when none is selected and X wraps to the first', async () => {
    native.entities = deployed();
    set(native.entities, 'CS_Unit2', [1, 1, 183, 130, 0, 55]);
    set(native.entities, 'CS_UnitAux2', [3, 3.8, 0, 0, 0, 0]);
    set(native.entities, 'CS_UnitCost2', [55, 46, 65, 1, 0, 0]);
    set(native.entities, 'CS_Unit9', [4, 1, 184, 130, 0, 110]);
    set(native.entities, 'CS_UnitAux9', [4, 4.1, 0, 0, 0, 0]);
    set(native.entities, 'CS_UnitCost9', [85, 70, 100, 1, 0, 0]);
    await enter(); key('z');
    expect(screen.getByRole('heading')).toHaveTextContent('GPT 娘');
    key('x'); expect(screen.getByRole('heading')).toHaveTextContent('VS Code');
    key('z'); expect(screen.getByRole('heading')).toHaveTextContent('GPT 娘');
    expect(native.sendInput).not.toHaveBeenCalled();
  });

  it('V and Escape expose the grid without losing the last cell or accidentally deploying', async () => {
    await enter(); chooseHand('operators'); key('1');
    expect(document.querySelector('.cards-hand')).toHaveClass('is-collapsed');
    key('v'); expect(document.querySelector('.cards-hand')).toHaveClass('is-open');
    key('v'); expect(document.querySelector('.cards-hand')).toHaveClass('is-collapsed');
    key('Escape'); key('Escape');
    expect(document.querySelector('.cards-hand')).toHaveClass('is-collapsed');
    expect(document.querySelectorAll('.cards-cell-grid > button')).toHaveLength(336);
    expect(screen.getByTestId('v3-cell-335')).toBeEnabled();
    expect(native.sendInput).not.toHaveBeenCalled();
    key('4'); fireEvent.click(screen.getByTestId('v3-cell-335'));
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 1_004_335);
  });

  it('translates a card drag into the target cell command and rejects hardware dropped outside the base', async () => {
    await enter();
    drop({ kind: 'hardware', id: 2 }, 183);
    expect(native.sendInput).not.toHaveBeenCalled();
    expect(screen.getByRole('status')).toHaveTextContent('拖到左侧基地');
    drop({ kind: 'operator', id: 2 }, 159);
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 1_002_159);
    drop({ kind: 'hardware', id: 2 }, 169);
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 4_000_002);
    expect(screen.getByTestId('v3-credits')).toHaveTextContent('560');
  });

  it('accepts a hardware drag over a visible empty rack socket and installs into that exact socket', async () => {
    await enter();
    const socket = screen.getByRole('button', { name: '显卡插槽 8' });
    const over = createEvent.dragOver(socket, { dataTransfer: { types: [TACTICAL_CARD_MIME] } });
    fireEvent(socket, over);
    expect(over.defaultPrevented).toBe(true);
    drop({ kind: 'hardware', id: 2 }, 169, socket);
    expect(native.sendInput.mock.calls).toEqual([['cs', 4_000_072]]);
    expect(screen.getByTestId('v3-credits')).toHaveTextContent('560');
  });

  it('allows Enter to activate the focused GPU card before a second Enter buys that newly selected model', async () => {
    await enter();
    const card = screen.getByRole('button', { name: '选择 RTX 5070' }); card.focus();
    const activation = createEvent.keyDown(card, { key: 'Enter' }); fireEvent(card, activation);
    expect(activation.defaultPrevented).toBe(false);
    expect(native.sendInput).not.toHaveBeenCalled();
    // jsdom does not synthesize the browser's default keyboard click.
    fireEvent.click(card); key('Enter');
    expect(native.sendInput.mock.calls).toEqual([['cs', 4_000_002]]);
  });

  it('keeps flip controls and the help modal from issuing purchase or gameplay commands', async () => {
    await enter();
    fireEvent.click(screen.getByRole('button', { name: '翻转 RTX 5060 查看详情' }));
    expect(screen.getByRole('button', { name: '返回 RTX 5060 正面' })).toBeInTheDocument();
    expect(native.sendInput).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '作战手册' }));
    expect((document.querySelector('.cards-shell') as HTMLElement).inert).toBe(true);
    for (const value of ['1', 'Enter', 'q', 'e', 'r', 'n', ' ']) key(value);
    expect(native.sendInput).not.toHaveBeenCalled();
    expect(native.callTool.mock.calls.some(([name]) => name === 'play_pause')).toBe(false);
    key('Escape'); expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('never opens purchase/drop commands before the first complete native publication', async () => {
    const first = deferred<Publication>(); native.reads.push(first.promise);
    await enter();
    expect(screen.queryByTestId('v3-cell-159')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: /接入首个空槽/ })).toBeDisabled();
    key('1'); key('Enter'); drop({ kind: 'operator', id: 1 }, 159);
    expect(native.sendInput).not.toHaveBeenCalled();
    await act(async () => { first.resolve({ entities: initial() }); });
    expect(screen.getByTestId('v3-cell-159')).toBeEnabled();
  });
});

describe('useSentinelsRuntime API lifecycle', () => {
  it('serializes concurrent start requests and keeps send gated until complete native state', async () => {
    const scene = deferred<unknown>(), publication = deferred<Publication>();
    const original = native.callTool.getMockImplementation()!;
    native.callTool.mockImplementation((name: string, ...args: unknown[]) => name === 'scene_load' ? scene.promise : original(name, ...args));
    native.reads.push(publication.promise);
    const { result } = renderHook(() => useSentinelsRuntime()); await act(async () => {});
    let first!: Promise<boolean>;
    await act(async () => { first = result.current.start(); expect(await result.current.start()).toBe(false); });
    expect(native.callTool.mock.calls.filter(([name]) => name === 'scene_load')).toHaveLength(1);
    await act(async () => { scene.resolve({}); expect(await first).toBe(true); });
    await connect();
    act(() => { expect(result.current.send(4_000_001)).toBe(false); });
    expect(native.sendInput).not.toHaveBeenCalled();
    await act(async () => { publication.resolve({ entities: initial() }); });
    act(() => { expect(result.current.send(4_000_001)).toBe(true); });
    expect(native.sendInput.mock.calls).toEqual([['cs', 4_000_001]]);
  });

  it('rejects a late old poll after restart instead of resurrecting old money or defeat', async () => {
    native.entities = deployed();
    const { result } = renderHook(() => useSentinelsRuntime()); await act(async () => {});
    await act(async () => { await result.current.start(); }); await connect();
    const old = deferred<Publication>(); native.reads.push(old.promise); await publish();
    const stale = deployed(); set(stale, 'CS_State', [777, 0, 2, 3, 80, 1]);
    native.entities = initial(); const previous = native.streams[0];
    await act(async () => { await result.current.start(); }); await connect();
    await act(async () => { old.resolve({ entities: stale }); });
    expect(previous.close).toHaveBeenCalledOnce();
    expect(result.current.state).toMatchObject({ credits: 560, energy: 0, hp: 20, phase: 0, gpuCount: 0 });
    expect(result.current.nativeReady).toBe(true);
  });

  it('automatically resumes a delayed pause after victory and permits the real next-level command', async () => {
    native.entities = deployed(); set(native.entities, 'CS_State', [200, 20, 4, 1, 50, 1]);
    const { result } = renderHook(() => useSentinelsRuntime()); await act(async () => {});
    await act(async () => { await result.current.start(); }); await connect();
    const pause = deferred<unknown>(), resume = deferred<unknown>(), original = native.callTool.getMockImplementation()!;
    native.callTool.mockImplementation((name: string, ...args: unknown[]) => name === 'play_pause' ? pause.promise : name === 'play_resume' ? resume.promise : original(name, ...args));
    let pending!: Promise<void>; act(() => { pending = result.current.togglePause(); });
    set(native.entities, 'CS_State', [200, 20, 4, 2, 57, 1]);
    set(native.entities, 'CS_Campaign', [1, 4, 2, 2, 2, 169]); await publish();
    await act(async () => { pause.resolve({}); await pending; });
    expect(native.callTool.mock.calls.filter(([name]) => name === 'play_resume')).toHaveLength(1);
    await act(async () => { resume.resolve({}); });
    expect(result.current.paused).toBe(false);
    act(() => { expect(result.current.send(5_200_000)).toBe(true); });
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 5_200_000);
  });
});
