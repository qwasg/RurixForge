import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import CodeSentinelsPlayer from '@/views/CodeSentinelsPlayer';
import type { ViewportStreamOptions } from '@/lib/viewportStream';

type NativeEntity = {
  id: number;
  name: string;
  transform: { translation: number[]; scale: number[]; rotation: number[] };
};

const native = vi.hoisted(() => ({
  entities: [] as NativeEntity[],
  streams: [] as Array<{ options: ViewportStreamOptions; close: ReturnType<typeof vi.fn> }>,
  sendInput: vi.fn(() => true),
  callTool: vi.fn(),
  apiGet: vi.fn(),
  setActive: vi.fn(),
  loadFailure: null as Error | null,
}));

vi.mock('@/lib/forgeApi', () => ({ callTool: native.callTool, apiGet: native.apiGet }));
vi.mock('@/lib/workspaceStore', () => ({
  useWorkspaceStore: { getState: () => ({ setActive: native.setActive }) },
}));
vi.mock('@/lib/viewportStream', () => ({
  openViewportStream: (options: ViewportStreamOptions) => {
    const close = vi.fn();
    native.streams.push({ options, close });
    return { up: true, sendInput: native.sendInput, close, sendPointer: vi.fn(() => true),
      sendCamera: vi.fn(), setSelected: vi.fn(), resize: vi.fn() };
  },
}));

function entity(name: string, translation: number[], scale = [0, 0, 0]): NativeEntity {
  return { id: 1, name, transform: { translation, scale, rotation: [0, 0, 0, 1] } };
}

/** Controlled native publications, deliberately no browser combat or economy simulation. */
function freshSnapshot() {
  return [entity('CS_State', [420, 20, 0]), entity('CS_StateAux', [0, 0, 0], [0, 1, 0]),
    entity('CS_Meta', [0, 0, 0]),
    ...Array.from({ length: 12 }, (_, i) => entity(`CS_Cell${i}`, [0, 0, 0]))];
}

async function publish() {
  await act(async () => { await vi.advanceTimersByTimeAsync(500); });
}

async function enterGame() {
  render(<CodeSentinelsPlayer />);
  await act(async () => {});
  expect(screen.getByRole('button', { name: '部署防线' })).toBeEnabled();
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: '部署防线' })); });
  await act(async () => { native.streams.at(-1)!.options.onChannel?.(true, 'connected'); });
  expect(screen.getByTestId('cell-0')).toBeEnabled();
}

beforeEach(() => {
  vi.useFakeTimers();
  // These tests exercise input/state contracts, not raster delivery or layout.
  vi.stubGlobal('requestAnimationFrame', vi.fn(() => 1));
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
  native.entities = freshSnapshot();
  native.streams = [];
  native.loadFailure = null;
  native.sendInput.mockReset().mockReturnValue(true);
  native.setActive.mockReset();
  native.apiGet.mockReset().mockResolvedValue({ workspaces: [
    { id: 'sentinels-workspace', root: 'D:/RurixForge/projects/code-sentinels', name: 'Code Sentinels' },
  ] });
  native.callTool.mockReset().mockImplementation(async (name: string) => {
    if (name === 'scene_summary') return { playState: 'edit' };
    if (name === 'entity_list') return { entities: native.entities };
    if (name === 'scene_load' && native.loadFailure) {
      const failure = native.loadFailure;
      native.loadFailure = null;
      throw failure;
    }
    return {};
  });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('Code Sentinels native gameplay UI', () => {
  it('sends the selected purchase as one native integer command and trusts native failure feedback', async () => {
    await enterGame();
    fireEvent.click(screen.getByRole('button', { name: /GPT 娘/ }));
    fireEvent.click(screen.getByTestId('cell-5'));
    expect(native.sendInput.mock.calls).toEqual([['cs', 1054]]);
    expect(screen.getByTestId('game-energy')).toHaveTextContent('420');
    expect(screen.getByRole('status')).not.toHaveTextContent('单元部署成功');

    // The server rejects the purchase; a successful socket send must not buy locally.
    native.entities[0] = entity('CS_State', [40, 20, 0]);
    native.entities[2] = entity('CS_Meta', [1, 0, 0]);
    await publish();
    expect(screen.getByRole('status')).toHaveTextContent('算力不足');
    expect(screen.getByTestId('game-energy')).toHaveTextContent('40');
    expect(screen.getByTestId('cell-5')).toHaveTextContent('+');
  });

  it('shows the selected existing tower profile and upgrades that same native slot', async () => {
    native.entities[6] = entity('CS_Cell3', [2, 1, 98]);
    await enterGame();
    fireEvent.click(screen.getByRole('button', { name: /GPT 娘/ }));
    fireEvent.click(screen.getByTestId('cell-3'));
    expect(screen.getByRole('heading', { name: 'PyCharm' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /PyCharm/ })).toHaveAttribute('aria-pressed', 'true');
    expect(native.sendInput).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: /升级.*98/ }));
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 2003);
    fireEvent.click(screen.getByRole('button', { name: '回收' }));
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 3003);
  });

  it('keeps game shortcuts out of the help dialog and ignores held-key repeats', async () => {
    await enterGame();
    fireEvent.click(screen.getByRole('button', { name: '作战手册' }));
    expect(screen.getByRole('dialog', { name: '作战手册' })).toBeInTheDocument();
    for (const event of [{ key: 'q', code: 'KeyQ' }, { key: 'n', code: 'KeyN' },
      { key: '4', code: 'Digit4' }, { key: ' ', code: 'Space' }]) fireEvent.keyDown(document.body, event);
    expect(native.sendInput).not.toHaveBeenCalled();
    expect(native.callTool.mock.calls.some(([name]) => name === 'play_pause')).toBe(false);
    expect(screen.getByRole('heading', { name: 'VS Code' })).toBeInTheDocument();

    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    fireEvent.keyDown(document.body, { key: 'q', code: 'KeyQ', repeat: true });
    fireEvent.keyDown(document.body, { key: ' ', code: 'Space', repeat: true });
    expect(native.sendInput).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '下路' }));
    fireEvent.keyDown(document.body, { key: 'q', code: 'KeyQ' });
    expect(native.sendInput).toHaveBeenLastCalledWith('cs', 4002);
  });

  it('allows retry after a failed restart and does not retain the previous defeat state', async () => {
    await enterGame();
    const originalStream = native.streams[0];
    native.entities[0] = entity('CS_State', [155, 0, 3], [3, 18, 1]);
    await publish();
    expect(screen.getByRole('heading', { name: '主分支需要你的支援。' })).toBeInTheDocument();
    expect(screen.getByText(/通过 2 波/)).toBeInTheDocument();
    native.loadFailure = new Error('scene reload failed');
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: '重新部署' })); });
    expect(originalStream.close).toHaveBeenCalledOnce();
    expect(screen.getByRole('alert')).toHaveTextContent('scene reload failed');
    expect(screen.queryByTestId('cell-0')).not.toBeInTheDocument();

    native.entities = freshSnapshot();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: '重试' })); });
    await act(async () => { native.streams.at(-1)!.options.onChannel?.(true, 'connected'); });
    expect(native.streams).toHaveLength(2);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: '主分支需要你的支援。' })).not.toBeInTheDocument();
    expect(screen.getByTestId('game-energy')).toHaveTextContent('420');
    expect(screen.getByTestId('cell-0')).toBeEnabled();
  });

  it('reports an input dropped during disconnect instead of displaying purchase success', async () => {
    await enterGame();
    native.sendInput.mockReturnValueOnce(false);
    fireEvent.click(screen.getByTestId('cell-0'));
    expect(screen.getByRole('status')).toHaveTextContent('指令未发送');
    expect(screen.getByRole('status')).not.toHaveTextContent('部署成功');
    expect(screen.getByTestId('game-energy')).toHaveTextContent('420');
    expect(screen.getByTestId('cell-0')).toHaveTextContent('+');
  });
});
