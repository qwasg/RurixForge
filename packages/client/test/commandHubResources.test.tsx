import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import CommandHub from '@/components/game/CommandHub';
import CommandResourcePanel from '@/components/game/CommandResourcePanel';
import CodeSentinelsCommand from '@/views/CodeSentinelsCommand';
import { INITIAL_V4, COMMAND, type V4Building, type V4State } from '@/lib/sentinelsV4';
import { DEFAULT_COMMAND_SETTINGS, loadCommandSettings, saveCommandSettings } from '@/lib/commandSettings';

const runtime = vi.hoisted(() => ({ current: {} as Record<string, unknown> }));
vi.mock('@/lib/useSentinelsCommandRuntime', () => ({ useSentinelsCommandRuntime: () => runtime.current }));

function building(slot: number, kind: number, cell: number, fields: Partial<V4Building> = {}): V4Building {
  return { slot, kind, cell, tier: 1, hp: 250, powered: true, connected: true, owner: 0, demand: 0, supply: 0,
    active: true, rate: 0, repairCost: 20, range: 0, upgradeCost: 60, sellRefund: 80, targetCell: cell,
    x: cell % 32 - 15.5, y: 9.5 - Math.floor(cell / 32), ...fields };
}
function nativeState(): V4State {
  return { ...INITIAL_V4, credits: 765, compute: 84, capacity: 300, powerGenerated: 150, powerDemand: 195, production: 0,
    incomePerSecond: 6, tech: 1, unlocked: 2, spentAttack: 33, spentSkill: 55, spentShield: 12,
    terrain: Array(640).fill(0), wallCells: Array(640).fill(false), closedCells: Array(640).fill(false), shieldCells: Array(640).fill(false),
    buildings: [building(3, 3, 33, { supply: 150 }), building(7, 2, 65, { powered: false, connected: false, demand: 195 }),
      building(11, 9, 99, { rate: 6 }), building(14, 8, 164)],
    gpus: [{ slot: 4, centerSlot: 7, model: 1, tier: 1, rate: 12, demand: 55, active: true, powered: false,
      capacity: 300, invested: 130, upgradeCost: 50, sellRefund: 91, bay: 0 }],
  };
}
const callbacks = () => ({ onContinue: vi.fn(), onDeploy: vi.fn(), onMultiplayer: vi.fn(), onReconnect: vi.fn(), onSettings: vi.fn() });
beforeEach(() => {
  localStorage.clear(); vi.stubGlobal('AudioContext', undefined);
  runtime.current = { workspace: { id: 'command-fixture', root: 'D:/RurixForge/projects/code-sentinels' }, state: nativeState(),
    savedUnlocked: 3, animationState: null, ready: false, nativeReady: false, busy: false, connected: false, paused: false,
    fps: 40, error: '', notice: '', noticeRevision: 0, connectionEpoch: 0, canvasRef: { current: null },
    setNotice: vi.fn(), start: vi.fn(async () => true), send: vi.fn(() => true), sendMany: vi.fn(() => 0),
    togglePause: vi.fn(async () => {}), reconnect: vi.fn() };
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

function battlefieldClick(cell: number) {
  const board = document.querySelector<HTMLDivElement>('.command-board')!;
  board.getBoundingClientRect = () => ({ left: 0, top: 0, width: 1280, height: 720, x: 0, y: 0, right: 1280, bottom: 720, toJSON: () => ({}) });
  board.setPointerCapture = vi.fn(); board.hasPointerCapture = () => false;
  const worldWidth = 320 / 9, margin = (worldWidth - 32) / 2;
  const init = { bubbles: true, button: 0, clientX: (cell % 32 + .5 + margin) / worldWidth * 1280,
    clientY: (Math.floor(cell / 32) + .5) / 20 * 720 };
  for (const type of ['pointerdown', 'pointerup']) {
    const event = new MouseEvent(type, init); Object.defineProperty(event, 'pointerId', { value: 1 }); fireEvent(board, event);
  }
}

function beginPendingDataCenterLink() {
  fireEvent.click(screen.getByRole('button', { name: /DATA-CENTER/ }));
  battlefieldClick(130);
  expect(runtime.current.send).toHaveBeenCalledExactlyOnceWith(COMMAND.build(2, 130));
  fireEvent.click(screen.getByRole('button', { name: '连接电力线' }));
  battlefieldClick(33); battlefieldClick(163);
  expect(runtime.current.send).toHaveBeenCalledOnce();
  expect(runtime.current.setNotice).toHaveBeenLastCalledWith('建筑状态同步中，确认后继续连接；Esc 可取消');
}

describe('command hall actions and truthful campaign state', () => {
  it('allows inspecting a locked sector but never deploys it', () => {
    const actions = callbacks();
    render(<CommandHub state={nativeState()} active={false} busy={false} available connected={false} unlocked={1}
      settings={DEFAULT_COMMAND_SETTINGS} {...actions}/>);
    expect(screen.queryByRole('button', { name: /继续当前行动/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '查看递归高地，未解锁' }));
    expect(screen.getByText('通关前一战区后解锁')).toBeInTheDocument();
    const deploy = screen.getByRole('button', { name: '开始单人行动' }); expect(deploy).toBeDisabled();
    fireEvent.click(deploy); expect(actions.onDeploy).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '查看断点森林' })); fireEvent.click(deploy);
    expect(actions.onDeploy).toHaveBeenCalledExactlyOnceWith(1);
  });

  it('keeps continuing separate from redeploying an unlocked sector', () => {
    const actions = callbacks();
    render(<CommandHub state={nativeState()} active busy={false} available connected unlocked={3}
      settings={DEFAULT_COMMAND_SETTINGS} {...actions}/>);
    fireEvent.click(screen.getByRole('button', { name: /继续当前行动/ }));
    expect(actions.onContinue).toHaveBeenCalledOnce(); expect(actions.onDeploy).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '查看泄漏湿地' }));
    fireEvent.click(screen.getByRole('button', { name: '重新部署此战区' }));
    expect(actions.onDeploy).toHaveBeenCalledExactlyOnceWith(2);
    fireEvent.click(screen.getByRole('button', { name: /联机准备室/ })); expect(actions.onMultiplayer).toHaveBeenCalledOnce();
    expect(screen.getByText(/PVP 战斗尚未开放/)).toBeInTheDocument();
  });

  it('exposes actual GPU source links and changes display and audio preferences', () => {
    const actions = callbacks();
    render(<CommandHub state={nativeState()} active={false} busy={false} available connected={false} unlocked={1}
      settings={DEFAULT_COMMAND_SETTINGS} {...actions}/>);
    fireEvent.click(screen.getByRole('button', { name: /档案/ })); fireEvent.click(screen.getByRole('button', { name: '显卡硬件' }));
    expect(screen.getByRole('link', { name: /厂商资料与图片来源/ })).toHaveAttribute('href', expect.stringContaining('msi.com'));
    fireEvent.click(screen.getByRole('button', { name: /设置/ }));
    fireEvent.change(screen.getByRole('slider', { name: '界面音效音量' }), { target: { value: '65' } });
    expect(actions.onSettings).toHaveBeenLastCalledWith({ ...DEFAULT_COMMAND_SETTINGS, volume: .65 });
    fireEvent.change(screen.getByRole('combobox', { name: '默认覆盖图层' }), { target: { value: '2' } });
    expect(actions.onSettings).toHaveBeenLastCalledWith({ ...DEFAULT_COMMAND_SETTINGS, overlay: 2 });
    fireEvent.click(screen.getByRole('checkbox', { name: /启用战场快捷键/ }));
    expect(actions.onSettings).toHaveBeenLastCalledWith({ ...DEFAULT_COMMAND_SETTINGS, shortcuts: false });
  });
});

describe('resource telemetry and real facility navigation', () => {
  it('uses native totals and distinguishes offline GPU hardware from current production', () => {
    const onLocate = vi.fn(), onResearch = vi.fn(), state = nativeState();
    render(<CommandResourcePanel state={state} connected onClose={vi.fn()} onLocate={onLocate} onResearch={onResearch}/>);
    expect(screen.getByTestId('resource-credits')).toHaveTextContent('765');
    expect(screen.getByTestId('resource-power')).toHaveTextContent('150 / 195');
    expect(screen.getByText('缺口 45')).toBeInTheDocument();
    expect(screen.getByRole('meter', { name: '算力储量' })).toHaveAttribute('aria-valuenow', '84');
    expect(screen.getByText('0 块 GPU 获供电 · 3 个空槽')).toBeInTheDocument();
    const rack = screen.getByRole('button', { name: /定位B3机架 1 GeForce RTX 5060/ });
    expect(rack).toHaveTextContent('+0/s'); fireEvent.click(rack); expect(onLocate).toHaveBeenLastCalledWith(7);
    fireEvent.click(screen.getByRole('button', { name: '定位资源采集器 D4' })); expect(onLocate).toHaveBeenLastCalledWith(11);
    fireEvent.click(screen.getByRole('button', { name: /科技网络 · T1/ })); expect(onResearch).toHaveBeenCalledOnce();
    expect(state.credits).toBe(765); expect(state.compute).toBe(84);
  });

  it('updates only when another native snapshot arrives and shows disconnection without inventing values', () => {
    const props = { state: nativeState(), connected: true, onClose: vi.fn(), onLocate: vi.fn(), onResearch: vi.fn() };
    const view = render(<CommandResourcePanel {...props}/>);
    view.rerender(<CommandResourcePanel {...props} connected={false}/>);
    expect(screen.getByText(/连接中断 · 显示最后一次同步/)).toBeInTheDocument();
    expect(screen.getByTestId('resource-credits')).toHaveTextContent('765');
    view.rerender(<CommandResourcePanel {...props} state={{ ...props.state, credits: 801, compute: 112 }}/>);
    expect(screen.getByTestId('resource-credits')).toHaveTextContent('801');
    expect(screen.getByRole('meter')).toHaveAttribute('aria-valuenow', '112');
  });
});

describe('command view integration', () => {
  it('waits for native readiness before selecting a saved unlocked sector and sends it once', async () => {
    const view = render(<CodeSentinelsCommand version={5}/>);
    fireEvent.click(screen.getByRole('button', { name: '查看递归高地' }));
    await act(async () => fireEvent.click(screen.getByRole('button', { name: '开始单人行动' })));
    expect(runtime.current.start).toHaveBeenCalledOnce(); expect(runtime.current.send).not.toHaveBeenCalled();
    runtime.current = { ...runtime.current, ready: true, connected: true };
    view.rerender(<CodeSentinelsCommand version={5}/>); expect(runtime.current.send).not.toHaveBeenCalled();
    runtime.current = { ...runtime.current, nativeReady: true };
    view.rerender(<CodeSentinelsCommand version={5}/>);
    expect(runtime.current.send).toHaveBeenCalledExactlyOnceWith(COMMAND.selectLevel(3));
    view.rerender(<CodeSentinelsCommand version={5}/>); expect(runtime.current.send).toHaveBeenCalledOnce();
    expect((runtime.current.state as V4State).level).toBe(1);
    expect(screen.queryByRole('dialog', { name: '编译防线行动大厅' })).not.toBeInTheDocument();
  });

  it('returns from the hall without restarting the active match and locates its actual data center', async () => {
    runtime.current = { ...runtime.current, ready: true, nativeReady: true, connected: true };
    const view = render(<CodeSentinelsCommand version={5}/>);
    await act(async () => fireEvent.click(screen.getByRole('button', { name: '打开行动大厅' })));
    expect(runtime.current.togglePause).toHaveBeenCalledOnce();
    runtime.current = { ...runtime.current, paused: true }; view.rerender(<CodeSentinelsCommand version={5}/>);
    fireEvent.click(screen.getByRole('button', { name: /继续当前行动/ }));
    expect(runtime.current.togglePause).toHaveBeenCalledTimes(2); expect(runtime.current.start).not.toHaveBeenCalled(); expect(runtime.current.send).not.toHaveBeenCalled();
    runtime.current = { ...runtime.current, paused: false }; view.rerender(<CodeSentinelsCommand version={5}/>);
    fireEvent.click(screen.getByRole('button', { name: '查看算力与机架' }));
    const panel = screen.getByRole('dialog', { name: '资源控制面板' });
    fireEvent.click(within(panel).getAllByRole('button', { name: '定位数据中心 B3' })[0]);
    expect(screen.queryByRole('dialog', { name: '资源控制面板' })).not.toBeInTheDocument();
    expect(screen.getByRole('heading', { name: '数据中心' })).toBeInTheDocument();
    expect(runtime.current.send).not.toHaveBeenCalled();
  });

  it('releases a pending deployment after a native connection error so recovery stays available', async () => {
    const view = render(<CodeSentinelsCommand version={5}/>);
    fireEvent.click(screen.getByRole('button', { name: '查看递归高地' }));
    await act(async () => fireEvent.click(screen.getByRole('button', { name: '开始单人行动' })));
    runtime.current = { ...runtime.current, ready: true, error: '原生状态同步中断' };
    view.rerender(<CodeSentinelsCommand version={5}/>);
    const hub = screen.getByRole('dialog', { name: '编译防线行动大厅' });
    expect(within(hub).getByRole('alert')).toHaveTextContent('原生状态同步中断');
    const reconnect = within(hub).getByRole('button', { name: '重新接入' }); expect(reconnect).toBeEnabled();
    fireEvent.click(reconnect); expect(runtime.current.reconnect).toHaveBeenCalledOnce();
    expect(runtime.current.send).not.toHaveBeenCalled();
  });

  it('finishes the third sector without offering a fourth and returns to the hall without restarting', async () => {
    runtime.current = { ...runtime.current, ready: true, nativeReady: true, connected: true,
      state: { ...nativeState(), level: 2, phase: 2 } };
    const view = render(<CodeSentinelsCommand version={5}/>);
    fireEvent.click(screen.getByRole('button', { name: '下一战区' }));
    expect(runtime.current.send).toHaveBeenCalledExactlyOnceWith(COMMAND.nextLevel());
    runtime.current = { ...runtime.current, state: { ...nativeState(), level: 3, phase: 2, unlocked: 3 } };
    view.rerender(<CodeSentinelsCommand version={5}/>);
    expect(screen.getByRole('heading', { name: '战役完成' })).toBeInTheDocument();
    expect(screen.getByText(/三个战区已守住/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '下一战区' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '重新部署' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '查看战场' })).toBeInTheDocument();
    await act(async () => fireEvent.click(screen.getByRole('button', { name: '返回大厅' })));
    expect(screen.getByRole('dialog', { name: '编译防线行动大厅' })).toBeInTheDocument();
    expect(runtime.current.start).not.toHaveBeenCalled(); expect(runtime.current.togglePause).not.toHaveBeenCalled();
    expect(runtime.current.send).toHaveBeenCalledOnce();
    expect(runtime.current.state).toMatchObject({ level: 3, phase: 2, credits: 765 });
    fireEvent.click(screen.getByRole('button', { name: /返回战场复盘/ }));
    expect(screen.queryByRole('dialog', { name: '编译防线行动大厅' })).not.toBeInTheDocument();
    expect(runtime.current.start).not.toHaveBeenCalled(); expect(runtime.current.send).toHaveBeenCalledOnce();
  });
});

describe('local preferences contain no campaign save', () => {
  it('handles invalid stored values and persists only the defined preferences', () => {
    localStorage.setItem('code-sentinels-preferences-v1', '{bad'); expect(loadCommandSettings()).toEqual(DEFAULT_COMMAND_SETTINGS);
    localStorage.setItem('code-sentinels-preferences-v1', JSON.stringify({ volume: 3, overlay: 9, showNotices: false, credits: 999999, unlocked: 3 }));
    expect(loadCommandSettings()).toEqual({ ...DEFAULT_COMMAND_SETTINGS, volume: 1, showNotices: false });
    saveCommandSettings({ ...DEFAULT_COMMAND_SETTINGS, shortcuts: false });
    const saved = JSON.parse(localStorage.getItem('code-sentinels-preferences-v1')!);
    expect(saved.shortcuts).toBe(false); expect(saved).not.toHaveProperty('credits'); expect(saved).not.toHaveProperty('unlocked');
  });
});

describe('technology and freshly built line endpoints', () => {
  it('describes tech-locked GPU cards without falsely claiming insufficient funds', () => {
    runtime.current = { ...runtime.current, ready: true, nativeReady: true, connected: true,
      state: { ...nativeState(), credits: 1659, tech: 0 } };
    render(<CodeSentinelsCommand version={5}/>);
    fireEvent.click(screen.getByRole('button', { name: /^显卡\s*I$/ }));
    for (const model of ['RTX 5080', 'RTX 5090', 'A100']) {
      const card = screen.getByRole('button', { name: `选择 ${model}` });
      expect(card).toHaveAccessibleDescription(expect.stringContaining('科技尚未解锁'));
      expect(card).not.toHaveAccessibleDescription(expect.stringContaining('当前经费不足'));
      expect(card).toBeEnabled();
    }
    fireEvent.click(screen.getByRole('button', { name: '翻转 RTX 5080 查看详情' }));
    expect(screen.getByRole('button', { name: '返回 RTX 5080 正面' })).toBeInTheDocument();
    expect(runtime.current.send).not.toHaveBeenCalled();
  });

  it('waits for the new data center snapshot and connects its authoritative anchor once', () => {
    runtime.current = { ...runtime.current, ready: true, nativeReady: true, connected: true };
    const view = render(<CodeSentinelsCommand version={5}/>); beginPendingDataCenterLink();
    const snapshot = runtime.current.state as V4State;
    runtime.current = { ...runtime.current, state: { ...snapshot, buildings: [...snapshot.buildings, building(18, 2, 130)] } };
    view.rerender(<CodeSentinelsCommand version={5}/>);
    expect(runtime.current.send).toHaveBeenLastCalledWith(COMMAND.powerLine(33, 130));
    expect(runtime.current.send).toHaveBeenCalledTimes(2);
    view.rerender(<CodeSentinelsCommand version={5}/>); expect(runtime.current.send).toHaveBeenCalledTimes(2);
    expect(snapshot.credits).toBe(765); expect(snapshot.buildings.some(item => item.cell === 130)).toBe(false);
  });

  it('still permits a normal empty-ground extension while another endpoint is awaiting construction', () => {
    runtime.current = { ...runtime.current, ready: true, nativeReady: true, connected: true };
    render(<CodeSentinelsCommand version={5}/>); beginPendingDataCenterLink();
    battlefieldClick(200);
    expect(runtime.current.send).toHaveBeenLastCalledWith(COMMAND.powerLine(33, 200));
    expect(runtime.current.send).toHaveBeenCalledTimes(2);
  });

  it('also waits when the newly built data center is selected as the line origin', () => {
    runtime.current = { ...runtime.current, ready: true, nativeReady: true, connected: true };
    const view = render(<CodeSentinelsCommand version={5}/>);
    fireEvent.click(screen.getByRole('button', { name: /DATA-CENTER/ })); battlefieldClick(130);
    fireEvent.click(screen.getByRole('button', { name: '连接算力线' })); battlefieldClick(163);
    expect(runtime.current.send).toHaveBeenCalledOnce();
    const snapshot = runtime.current.state as V4State;
    runtime.current = { ...runtime.current, state: { ...snapshot, buildings: [...snapshot.buildings, building(18, 2, 130)] } };
    view.rerender(<CodeSentinelsCommand version={5}/>);
    expect(runtime.current.send).toHaveBeenCalledOnce();
    battlefieldClick(164); expect(runtime.current.send).toHaveBeenLastCalledWith(COMMAND.computeLine(130, 164));
  });

  it.each(['cancel', 'tool', 'disconnect'] as const)('cancels an unsent dependent line on %s', reason => {
    runtime.current = { ...runtime.current, ready: true, nativeReady: true, connected: true };
    const view = render(<CodeSentinelsCommand version={5}/>); beginPendingDataCenterLink();
    if (reason === 'cancel') fireEvent.keyDown(window, { key: 'Escape' });
    else if (reason === 'tool') fireEvent.click(screen.getByRole('button', { name: '连接算力线' }));
    else { runtime.current = { ...runtime.current, connected: false }; view.rerender(<CodeSentinelsCommand version={5}/>); }
    const snapshot = runtime.current.state as V4State;
    runtime.current = { ...runtime.current, connected: true, state: { ...snapshot, buildings: [...snapshot.buildings, building(18, 2, 130)] } };
    view.rerender(<CodeSentinelsCommand version={5}/>);
    expect(runtime.current.send).toHaveBeenCalledOnce();
  });

  it('expires an unconfirmed building without sending a line and allows a later ground extension', () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    const now = vi.spyOn(performance, 'now').mockReturnValue(0);
    runtime.current = { ...runtime.current, ready: true, nativeReady: true, connected: true };
    render(<CodeSentinelsCommand version={5}/>); beginPendingDataCenterLink();
    now.mockReturnValue(5001); act(() => vi.advanceTimersByTime(5001));
    expect(runtime.current.send).toHaveBeenCalledOnce();
    expect(runtime.current.setNotice).toHaveBeenLastCalledWith('尚未确认该建筑，连线未发送；请确认建造结果后重试');
    battlefieldClick(163); expect(runtime.current.send).toHaveBeenLastCalledWith(COMMAND.powerLine(33, 163));
  });

  it('rejects a confirmation past the absolute deadline even if a throttled timer has not fired', () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    const now = vi.spyOn(performance, 'now').mockReturnValue(0);
    runtime.current = { ...runtime.current, ready: true, nativeReady: true, connected: true };
    const view = render(<CodeSentinelsCommand version={5}/>); beginPendingDataCenterLink();
    now.mockReturnValue(5001);
    // Deliberately leave the scheduled timer pending, as in a background tab.
    const snapshot = runtime.current.state as V4State;
    runtime.current = { ...runtime.current, state: { ...snapshot, buildings: [...snapshot.buildings, building(18, 2, 130)] } };
    view.rerender(<CodeSentinelsCommand version={5}/>);
    expect(runtime.current.send).toHaveBeenCalledExactlyOnceWith(COMMAND.build(2, 130));
    expect(runtime.current.setNotice).toHaveBeenLastCalledWith('尚未确认该建筑，连线未发送；请确认建造结果后重试');
    act(() => vi.advanceTimersByTime(5001)); expect(runtime.current.send).toHaveBeenCalledOnce();
  });
});
