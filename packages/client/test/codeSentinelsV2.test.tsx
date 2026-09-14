import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import CodeSentinelsV2 from '@/views/CodeSentinelsV2';
import type { ViewportStreamOptions } from '@/lib/viewportStream';
import captured from './fixtures/sentinels-v2-native.json';

type Entity = { name: string; transform: { translation: number[]; scale: number[] } };
type Publication = { entities: Entity[] };
type Stream = { options: ViewportStreamOptions; up: boolean; close: ReturnType<typeof vi.fn> };
const native = vi.hoisted(() => ({
  entities: [] as Entity[], streams: [] as Stream[], reads: [] as Promise<Publication>[],
  sendInput: vi.fn(() => true), callTool: vi.fn(), apiGet: vi.fn(), setActive: vi.fn(),
  loadFailure: null as Error | null,
}));
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
  const record = entities.find((entity) => entity.name === name)!;
  record.transform = { translation: fields.slice(0, 3), scale: fields.slice(3, 6) };
}
function clearUnit(entities: Entity[], slot: number) {
  set(entities, `CS_Unit${slot}`, [0, 0, -1, 0, 0, 0]);
  set(entities, `CS_UnitAux${slot}`, [0, 0, 0, 0, 0, 0]);
  set(entities, `CS_UnitCost${slot}`, [0, 0, 0, 0, 0, 0]);
}
/** Controlled native publications, based on the captured DLL format; no browser economy simulation. */
function initialPublication() {
  const entities = clone();
  set(entities, 'CS_State', [0, 20, 0, 0, 0, 1]);
  set(entities, 'CS_Economy', [560, 0, 0, 0, 0, 0]);
  set(entities, 'CS_Meta', [0, 0, 0, 0, 0, 0]);
  clearUnit(entities, 0);
  set(entities, 'CS_GPU0', [0, 0, 0, 0, 0, 0]);
  return entities;
}
function deployedPublication() {
  const entities = clone();
  clearUnit(entities, 0);
  // Values deliberately differ from catalogue defaults: UI must use this publication.
  set(entities, 'CS_State', [200, 20, 0, 0, 0, 1]);
  set(entities, 'CS_Meta', [0, 0, 2, 0, 0, 0]);
  set(entities, 'CS_Unit6', [3, 2, 159, 140, 0, 97]);
  set(entities, 'CS_UnitAux6', [7.25, 6.75, 0, 14, 0, 0]);
  set(entities, 'CS_UnitCost6', [116, 182, 260, 2, 0, 0]);
  return entities;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
async function connect(stream = native.streams.at(-1)!) {
  await act(async () => { stream.up = true; stream.options.onChannel?.(true, 'connected'); });
}
async function enterGame() {
  render(<CodeSentinelsV2 />);
  await act(async () => {});
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: /启动算力前线/ })); });
  await connect();
}
async function publish() {
  await act(async () => { await vi.advanceTimersByTimeAsync(400); });
}
const key = (name: string, repeat = false) => fireEvent.keyDown(document.body,
  { key: name, code: name === ' ' ? 'Space' : `Key${name.toUpperCase()}`, repeat });

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal('requestAnimationFrame', vi.fn(() => 1));
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
  history.replaceState({}, '', '/play/code-sentinels-v2?standalone=1');
  native.entities = initialPublication(); native.streams = []; native.reads = []; native.loadFailure = null;
  native.sendInput.mockReset().mockReturnValue(true); native.setActive.mockReset();
  native.apiGet.mockReset().mockResolvedValue({ workspaces: [
    { id: 'v2-workspace', name: '编译防线', root: 'D:/RurixForge/projects/code-sentinels' },
  ] });
  native.callTool.mockReset().mockImplementation(async (name: string) => {
    if (name === 'scene_summary') return { playState: 'edit' };
    if (name === 'entity_list') return native.reads.shift() ?? { entities: structuredClone(native.entities) };
    if (name === 'scene_load' && native.loadFailure) {
      const error = native.loadFailure; native.loadFailure = null; throw error;
    }
    return {};
  });
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe('Code Sentinels V2 native UI contracts', () => {
  it('starts at zero compute and sends GPU purchase 4000001 without inventing local production', async () => {
    await enterGame();
    expect(screen.getByTestId('v2-energy')).toHaveTextContent(/^0 \/ 0$/);
    expect(screen.getByTestId('v2-production')).toHaveTextContent('+0');
    expect(screen.getByText('0 / 8 在线')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /安装到基地/ }));
    expect(native.sendInput.mock.calls).toEqual([['cs', 4_000_001]]);
    expect(screen.getByTestId('v2-credits')).toHaveTextContent('560');
    expect(screen.getByTestId('v2-production')).toHaveTextContent('+0');
    expect(screen.getByRole('status')).not.toHaveTextContent('显卡上线');

    // This is the real captured native publication, including a subsequent deployment.
    native.entities = clone();
    await publish();
    expect(screen.getByTestId('v2-credits')).toHaveTextContent('365');
    expect(screen.getByTestId('v2-energy')).toHaveTextContent(/^24 \/ 300$/);
    expect(screen.getByTestId('v2-production')).toHaveTextContent('+12');
    expect(screen.getByText('1 / 8 在线')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '显卡插槽 1' })).toHaveTextContent('+12/s');
  });

  it('deploys into a free native cell with 1001000 + cell and preserves native rejection feedback', async () => {
    native.entities = clone(); clearUnit(native.entities, 0);
    await enterGame();
    fireEvent.click(screen.getByTestId('v2-cell-159'));
    expect(native.sendInput.mock.calls).toEqual([['cs', 1_001_159]]);
    expect(screen.getByText('0 / 24')).toBeInTheDocument();
    expect(screen.getByTestId('v2-credits')).toHaveTextContent('365');
    set(native.entities, 'CS_Economy', [42, 12, 300, 0, 0, 1]);
    set(native.entities, 'CS_Meta', [1, 65, 3, 0, 0, 0]);
    await publish();
    expect(screen.getByRole('status')).toHaveTextContent('经费不足');
    expect(screen.getByTestId('v2-credits')).toHaveTextContent('42');
    expect(screen.getByText('0 / 24')).toBeInTheDocument();
  });

  it('selects the existing native slot, then Q and one target send one high-cost skill command', async () => {
    native.entities = deployedPublication();
    await enterGame();
    fireEvent.click(screen.getByRole('button', { name: '选择 PyCharm' }));
    fireEvent.click(screen.getByTestId('v2-cell-159'));
    expect(screen.getByRole('heading', { name: 'DeepSeek 娘' })).toBeInTheDocument();
    expect(screen.getByText('97 算力')).toBeInTheDocument();
    expect(screen.getByText('6.8 格')).toBeInTheDocument();
    expect(native.sendInput).not.toHaveBeenCalled();
    key('q'); key('q', true);
    expect(screen.getByTestId('v2-board')).toHaveClass('is-aiming');
    fireEvent.click(screen.getByTestId('v2-cell-183'));
    expect(native.sendInput.mock.calls).toEqual([['cs', 3_006_183]]);
    expect(screen.getByTestId('v2-board')).not.toHaveClass('is-aiming');
    expect(screen.getByTestId('v2-energy')).toHaveTextContent(/^200/);
    expect(screen.getByRole('status')).not.toHaveTextContent('算力已扣除');

    set(native.entities, 'CS_State', [103, 20, 0, 0, 0, 1]);
    set(native.entities, 'CS_Economy', [365, 12, 300, 0, 97, 1]);
    set(native.entities, 'CS_Unit6', [3, 2, 159, 140, 8.25, 97]);
    set(native.entities, 'CS_Meta', [13, 97, 3, 0, 0, 0]);
    await publish();
    expect(screen.getByTestId('v2-energy')).toHaveTextContent(/^103/);
    expect(screen.getByRole('button', { name: /9s 后可释放/ })).toBeDisabled();
    key('q');
    expect(native.sendInput).toHaveBeenCalledTimes(1);
  });

  it('keeps Q, N, number keys and pause outside modal dialogs and cancels aim on Escape', async () => {
    native.entities = deployedPublication();
    await enterGame(); fireEvent.click(screen.getByTestId('v2-cell-159'));
    fireEvent.click(screen.getByRole('button', { name: '作战手册' }));
    expect(screen.getByRole('dialog', { name: '作战手册' })).toBeInTheDocument();
    expect((document.querySelector('.v2-shell') as HTMLElement).inert).toBe(true);
    for (const name of ['q', 'n', '4', ' ']) key(name);
    expect(native.sendInput).not.toHaveBeenCalled();
    expect(native.callTool.mock.calls.some(([name]) => name === 'play_pause')).toBe(false);
    expect(screen.getByTestId('v2-board')).not.toHaveClass('is-aiming');
    key('Escape'); key('q');
    expect(screen.getByTestId('v2-board')).toHaveClass('is-aiming');
    key('Escape');
    expect(screen.getByTestId('v2-board')).not.toHaveClass('is-aiming');
  });

  it('opens supply controls by clicking the native base and targets the chosen GPU socket', async () => {
    native.entities = deployedPublication();
    await enterGame(); fireEvent.click(screen.getByTestId('v2-cell-159'));
    expect(screen.getByRole('heading', { name: 'DeepSeek 娘' })).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('v2-cell-169'));
    expect(screen.getByText('基地机架')).toBeInTheDocument();
    expect(native.sendInput).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '显卡插槽 8' }));
    fireEvent.click(screen.getByRole('button', { name: /选择 GeForce RTX 5070/ }));
    fireEvent.click(screen.getByRole('button', { name: /安装到基地/ }));
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 4_000_072);
    expect(screen.getByRole('button', { name: '显卡插槽 8' })).toHaveTextContent('08');
    expect(screen.getByText('1 / 8 在线')).toBeInTheDocument();
  });

  it('uses native fractional production, GPU upgrade prices and refunds for the selected socket', async () => {
    native.entities = clone();
    set(native.entities, 'CS_Economy', [365, 61.6, 1080, 0, 0, 2]);
    set(native.entities, 'CS_GPU7', [3, 2, 49.6, 350, 315, 416]);
    await enterGame();
    expect(screen.getByTestId('v2-production')).toHaveTextContent('+61.6');
    fireEvent.click(screen.getByRole('button', { name: '显卡插槽 8' }));
    expect(screen.getByRole('button', { name: '显卡插槽 8' })).toHaveTextContent('+49.6/s');
    expect(screen.getByRole('button', { name: '升级显卡' })).toHaveTextContent('315');
    fireEvent.click(screen.getByRole('button', { name: '升级显卡' }));
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 4_100_007);
    expect(screen.getByTestId('v2-credits')).toHaveTextContent('365');
    set(native.entities, 'CS_Economy', [50, 79.2, 1260, 0, 0, 2]);
    set(native.entities, 'CS_GPU7', [3, 3, 67.2, 350, 0, 637]);
    await publish();
    expect(screen.getByTestId('v2-production')).toHaveTextContent('+79.2');
    expect(screen.getByRole('button', { name: '升级显卡' })).toBeDisabled();
    expect(screen.getByRole('button', { name: '回收显卡' })).toHaveAttribute('title', '回收返还 637 经费');
    fireEvent.click(screen.getByRole('button', { name: '回收显卡' }));
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 4_200_007);
    expect(screen.getByTestId('v2-credits')).toHaveTextContent('50');
  });

  it('gates a skill using the current native cost rather than the cheaper catalogue default', async () => {
    native.entities = deployedPublication();
    set(native.entities, 'CS_State', [96, 20, 0, 0, 0, 1]);
    await enterGame(); fireEvent.click(screen.getByTestId('v2-cell-159'));
    expect(screen.getByRole('button', { name: /选择技能落点/ })).toBeDisabled();
    key('q');
    expect(screen.getByTestId('v2-board')).not.toHaveClass('is-aiming');
    expect(native.sendInput).not.toHaveBeenCalled();
    expect(screen.getByRole('status')).toHaveTextContent('97 算力');
  });

  it('keeps native rejection feedback visible even while an existing unit is starved', async () => {
    native.entities = deployedPublication();
    set(native.entities, 'CS_UnitAux6', [7.25, 6.75, 0, 14, 0, 1]);
    await enterGame();
    fireEvent.click(screen.getByRole('button', { name: '显卡插槽 8' }));
    fireEvent.click(screen.getByRole('button', { name: /安装到基地/ }));
    set(native.entities, 'CS_Economy', [70, 12, 300, 0, 0, 1]);
    set(native.entities, 'CS_Meta', [1, 130, 3, 0, 0, 0]);
    await publish();
    expect(screen.getByRole('status')).toHaveTextContent('经费不足');
    expect(screen.getByText('1 / 8 在线')).toBeInTheDocument();
    expect(screen.getByTestId('v2-credits')).toHaveTextContent('70');
  });

  it('waits for native wave and level publications rather than changing campaign resources on send', async () => {
    native.entities = clone();
    set(native.entities, 'CS_Campaign', [1, 4, 2, 0, 2, 169]);
    await enterGame();
    fireEvent.click(screen.getByRole('button', { name: /开启下一波/ }));
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 5_000_000);
    expect(screen.getByTestId('v2-wave')).toHaveTextContent(/^00/);
    set(native.entities, 'CS_State', [20, 19, 1, 1, 2, 1]);
    await publish();
    expect(screen.getByTestId('v2-wave')).toHaveTextContent(/^01/);
    expect(screen.getByRole('button', { name: /开启下一波/ })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: /泄漏湿地/ }));
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 5_100_002);
    expect(screen.getByTestId('v2-credits')).toHaveTextContent('365');

    set(native.entities, 'CS_State', [7, 20, 0, 0, 2, 2]);
    set(native.entities, 'CS_Economy', [510, 12, 300, 18, 90, 1]);
    set(native.entities, 'CS_Campaign', [2, 4, 2, 0, 3, 169]);
    set(native.entities, 'CS_StateAux', [0, 0, 0, 1, 0, 3]);
    for (let row = 0; row < 14; row++) {
      const map = native.entities.find((entity) => entity.name === `CS_MapRow${row}`)!;
      map.transform.scale[1] = 3; map.transform.scale[2] = 2;
    }
    await publish();
    expect(screen.getByTestId('v2-credits')).toHaveTextContent('510');
    expect(screen.getByTestId('v2-energy')).toHaveTextContent(/^7 \/ 300$/);
    expect(screen.getByRole('button', { name: /泄漏湿地/ })).toHaveClass('is-current');
  });

  it('ignores a late pre-restart native poll and supports recovery after a failed scene reload', async () => {
    native.entities = clone();
    await enterGame();
    set(native.entities, 'CS_State', [155, 0, 2, 3, 18, 1]);
    await publish();
    expect(screen.getByRole('heading', { name: '核心失守' })).toBeInTheDocument();
    const stale = deferred<Publication>(); native.reads.push(stale.promise);
    await publish();
    const previous = native.streams[0];
    native.loadFailure = new Error('native scene reload failed');
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: '重新部署' })); });
    expect(previous.close).toHaveBeenCalledOnce();
    expect(screen.getByRole('alert')).toHaveTextContent('native scene reload failed');
    expect(screen.queryByTestId('v2-cell-159')).not.toBeInTheDocument();

    const defeat = structuredClone(native.entities);
    native.entities = initialPublication();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: '重新接入' })); });
    await connect();
    await act(async () => { stale.resolve({ entities: defeat }); });
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: '核心失守' })).not.toBeInTheDocument();
    expect(screen.getByTestId('v2-energy')).toHaveTextContent(/^0 \/ 0$/);
    expect(screen.getByTestId('v2-credits')).toHaveTextContent('560');
    expect(screen.getByTestId('v2-cell-159')).toBeEnabled();
  });

  it('resumes a late pause acknowledgement after native victory so the next-level command is not locked out', async () => {
    native.entities = clone();
    set(native.entities, 'CS_State', [250, 20, 4, 1, 50, 1]);
    const pause = deferred<unknown>(), resume = deferred<unknown>();
    const normalCall = native.callTool.getMockImplementation()!;
    native.callTool.mockImplementation((name: string, ...args: unknown[]) => {
      if (name === 'play_pause') return pause.promise;
      if (name === 'play_resume') return resume.promise;
      return normalCall(name, ...args);
    });
    await enterGame();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: '暂停战斗' })); });
    expect(native.callTool.mock.calls.filter(([name]) => name === 'play_pause')).toHaveLength(1);

    // The final Boss dies while the pause RPC acknowledgement is still in flight.
    set(native.entities, 'CS_State', [250, 20, 4, 2, 57, 1]);
    set(native.entities, 'CS_Campaign', [1, 4, 2, 2, 4, 169]);
    set(native.entities, 'CS_StateAux', [0, 0, 0, 1, 0, 4]);
    for (let row = 0; row < 14; row++) {
      native.entities.find((entity) => entity.name === `CS_MapRow${row}`)!.transform.scale[1] = 4;
    }
    await publish();
    expect(screen.getByRole('heading', { name: '战区已净化' })).toBeInTheDocument();
    expect(native.callTool.mock.calls.filter(([name]) => name === 'play_resume')).toHaveLength(0);

    await act(async () => { pause.resolve({}); });
    expect(native.callTool.mock.calls.filter(([name]) => name === 'play_resume')).toHaveLength(1);
    expect(native.sendInput).not.toHaveBeenCalled();
    // Repeated publication of the same victory must not issue repeated resumes.
    await publish();
    expect(native.callTool.mock.calls.filter(([name]) => name === 'play_resume')).toHaveLength(1);
    await act(async () => { resume.resolve({}); });
    expect(screen.getByRole('button', { name: '暂停战斗' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /带着显卡进入下一战区/ }));
    expect(native.sendInput.mock.calls).toEqual([['cs', 5_200_000]]);
    expect(screen.getByTestId('v2-energy')).toHaveTextContent(/^250/);
  });

  it('does not allow purchase commands before the first complete native state arrives', async () => {
    const publication = deferred<Publication>(); native.reads.push(publication.promise);
    await enterGame();
    expect(screen.getByRole('button', { name: /安装到基地/ })).toBeDisabled();
    expect(screen.queryByTestId('v2-cell-159')).not.toBeInTheDocument();
    key('n');
    expect(native.sendInput).not.toHaveBeenCalled();
    await act(async () => { publication.resolve({ entities: initialPublication() }); });
    expect(screen.getByRole('button', { name: /安装到基地/ })).toBeEnabled();
  });

  it('cancels pending skill targeting when the native selected unit ceases to exist', async () => {
    native.entities = deployedPublication();
    await enterGame(); fireEvent.click(screen.getByTestId('v2-cell-159')); key('q');
    expect(screen.getByTestId('v2-board')).toHaveClass('is-aiming');
    clearUnit(native.entities, 6);
    await publish();
    expect(screen.getByTestId('v2-board')).not.toHaveClass('is-aiming');
    expect(native.sendInput).not.toHaveBeenCalled();
  });

  it('shows a damaged upgraded unit below full health rather than treating 130 as every tier maximum', async () => {
    native.entities = deployedPublication();
    await enterGame(); fireEvent.click(screen.getByTestId('v2-cell-159'));
    // Native level 2 maximum is 175 HP, so this observed 140 HP is 80% health.
    expect(document.querySelector('.v2-hp-track > i')).toHaveStyle({ width: '80%' });
  });
});
