import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  FRAME_HEADER_LEN,
  mapKeyToInput,
  openViewportStream,
  parseFrameMessage,
} from '@/lib/viewportStream';
import { mockForgeBackend } from './forgeMock';

/**
 * 视口直连推流通道(client 侧):
 * - 二进制帧头解析(小端布局与 engine-host stream.rs encode_frame 逐字对拍);
 * - play 态键盘 → 输入契约映射(value 符号编码轴向,与现有游戏图兼容);
 * - 通道生命周期:连接→subscribe→帧/状态/错误分发→断流退避→onChannel 如实回调。
 */

/** 服务端 encode_frame 的镜像(测试构造帧消息) */
function buildFrame(opts: {
  frameId: number;
  width: number;
  height: number;
  flags: number;
  draws: number;
}): ArrayBuffer {
  const { frameId, width, height, flags, draws } = opts;
  const buf = new ArrayBuffer(FRAME_HEADER_LEN + width * height * 4);
  const dv = new DataView(buf);
  dv.setUint8(0, 0x46); // F
  dv.setUint8(1, 0x47); // G
  dv.setUint8(2, 0x46); // F
  dv.setUint8(3, 0x31); // 1
  dv.setUint32(4, frameId, true);
  dv.setUint16(8, width, true);
  dv.setUint16(10, height, true);
  dv.setUint32(12, flags, true);
  dv.setUint32(16, draws, true);
  new Uint8Array(buf, FRAME_HEADER_LEN).fill(9);
  return buf;
}

describe('parseFrameMessage:二进制帧头(与服务端 20B 小端布局对拍)', () => {
  it('合法帧:字段逐一解出,rgba 为负载零拷贝视图', () => {
    const f = parseFrameMessage(
      buildFrame({ frameId: 42, width: 8, height: 4, flags: 0b101, draws: 3 }),
    )!;
    expect(f).not.toBeNull();
    expect(f.frameId).toBe(42);
    expect(f.width).toBe(8);
    expect(f.height).toBe(4);
    expect(f.playing).toBe(true);
    expect(f.truncated).toBe(false);
    expect(f.imported).toBe(true);
    expect(f.draws).toBe(3);
    expect(f.rgba.length).toBe(8 * 4 * 4);
    expect(f.rgba[0]).toBe(9);
  });

  it('坏魔数 / 负载长度不符 / 短于头长 → null(不伪造帧)', () => {
    const bad = buildFrame({ frameId: 1, width: 2, height: 2, flags: 0, draws: 0 });
    new DataView(bad).setUint32(0, 0xdeadbeef, true);
    expect(parseFrameMessage(bad)).toBeNull();
    const truncated = buildFrame({ frameId: 1, width: 2, height: 2, flags: 0, draws: 0 }).slice(
      0,
      FRAME_HEADER_LEN + 3,
    );
    expect(parseFrameMessage(truncated)).toBeNull();
    expect(parseFrameMessage(new ArrayBuffer(4))).toBeNull();
  });
});

describe('mapKeyToInput:play 态键盘 → 引擎输入契约', () => {
  it('方向键/WASD/空格映射;value 符号编码轴向(左/下 -1,右/上 +1)', () => {
    expect(mapKeyToInput('ArrowLeft')).toEqual({ action: 'left', value: -1 });
    expect(mapKeyToInput('a')).toEqual({ action: 'left', value: -1 });
    expect(mapKeyToInput('ArrowRight')).toEqual({ action: 'right', value: 1 });
    expect(mapKeyToInput('D')).toEqual({ action: 'right', value: 1 });
    expect(mapKeyToInput('ArrowUp')).toEqual({ action: 'up', value: 1 });
    expect(mapKeyToInput('w')).toEqual({ action: 'up', value: 1 });
    expect(mapKeyToInput('ArrowDown')).toEqual({ action: 'down', value: -1 });
    expect(mapKeyToInput('s')).toEqual({ action: 'down', value: -1 });
    expect(mapKeyToInput(' ')).toEqual({ action: 'space', value: 1 });
  });

  it('未映射键 → null(编辑器快捷键不受劫持)', () => {
    expect(mapKeyToInput('e')).toBeNull();
    expect(mapKeyToInput('Escape')).toBeNull();
    expect(mapKeyToInput('F5')).toBeNull();
  });
});

/** 最小 WebSocket 桩:记录出站消息,可注入 open/message/close 事件 */
class MockWebSocket {
  static OPEN = 1;
  static instances: MockWebSocket[] = [];
  url: string;
  binaryType = 'blob';
  readyState = 0;
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(url: string) {
    this.url = url;
    MockWebSocket.instances.push(this);
  }
  send(data: string) {
    this.sent.push(data);
  }
  close() {
    this.readyState = 3;
    this.onclose?.();
  }
  /** 测试注入:服务端握手完成 */
  emitOpen() {
    this.readyState = 1;
    this.onopen?.();
  }
  emitMessage(data: unknown) {
    this.onmessage?.({ data });
  }
  emitClose() {
    this.readyState = 3;
    this.onclose?.();
  }
}

describe('openViewportStream:通道生命周期', () => {
  beforeEach(() => {
    MockWebSocket.instances = [];
    vi.stubGlobal('WebSocket', MockWebSocket);
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  function stubStreamInfo() {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend({
        viewport_stream_info: { wsUrl: 'ws://127.0.0.1:9/stream?token=tok', proto: 1 },
      }),
    );
  }

  it('连接→subscribe→hello/帧/状态/错误分发;输入与选中经出站消息回传', async () => {
    stubStreamInfo();
    const seen = {
      frames: [] as number[],
      status: [] as string[],
      errors: [] as string[],
      channel: [] as boolean[],
    };
    const h = openViewportStream({
      width: 320,
      height: 180,
      maxFps: 30,
      onFrame: (f) => seen.frames.push(f.frameId),
      onStatus: (s) => seen.status.push(s.playState),
      onError: (m) => seen.errors.push(m),
      onChannel: (up) => seen.channel.push(up),
    });
    await vi.advanceTimersByTimeAsync(0); // stream_info 拉取 + WS 构造
    const ws = MockWebSocket.instances[0];
    expect(ws).toBeDefined();
    expect(ws.url).toBe('ws://127.0.0.1:9/stream?token=tok');
    expect(ws.binaryType).toBe('arraybuffer');

    ws.emitOpen();
    expect(seen.channel).toEqual([true]);
    expect(JSON.parse(ws.sent[0])).toEqual({
      type: 'subscribe',
      width: 320,
      height: 180,
      maxFps: 30,
    });

    ws.emitMessage(JSON.stringify({ type: 'hello', proto: 1 }));
    ws.emitMessage(buildFrame({ frameId: 7, width: 2, height: 2, flags: 1, draws: 1 }));
    ws.emitMessage(JSON.stringify({ type: 'status', playState: 'play_running', fps: 60 }));
    ws.emitMessage(JSON.stringify({ type: 'error', message: 'DEV_ENV_DEGRADE: x' }));
    expect(seen.frames).toEqual([7]);
    expect(seen.status).toEqual(['play_running']);
    expect(seen.errors).toEqual(['DEV_ENV_DEGRADE: x']);

    h.sendInput('left', -1);
    h.sendPointer('click', 0.25, 0.75);
    h.setSelected(5);
    h.resize(640, 360);
    h.sendCamera({ yaw: 10 });
    const outbound = ws.sent.slice(1).map((s) => JSON.parse(s));
    expect(outbound).toEqual([
      { type: 'input', action: 'left', value: -1 },
      // 指针点击带归一化坐标(引擎反投影为世界坐标派发 click_x/click_y/click_z + click)
      { type: 'pointer', action: 'click', x: 0.25, y: 0.75 },
      { type: 'select', id: 5 },
      { type: 'resize', width: 640, height: 360 },
      { type: 'camera', yaw: 10 },
    ]);

    h.close();
    expect(ws.readyState).toBe(3);
  });

  it('断流退避:三连败 onChannel(false);恢复后重发 subscribe 并 onChannel(true)', async () => {
    stubStreamInfo();
    const channel: boolean[] = [];
    const h = openViewportStream({
      width: 100,
      height: 100,
      onFrame: () => {},
      onChannel: (up) => channel.push(up),
    });
    await vi.advanceTimersByTimeAsync(0);
    // 第 1 连:成功后被服务端断开 → 失败 1
    MockWebSocket.instances[0].emitOpen();
    expect(channel).toEqual([true]);
    MockWebSocket.instances[0].emitClose();
    // 退避 500ms → 第 2 连失败;1000ms → 第 3 连失败 → 三连败降通道
    await vi.advanceTimersByTimeAsync(500);
    MockWebSocket.instances[1].emitClose();
    await vi.advanceTimersByTimeAsync(1000);
    MockWebSocket.instances[2].emitClose();
    expect(channel).toEqual([true, false]);
    // 后台续试成功 → 恢复 onChannel(true) + 重新 subscribe
    await vi.advanceTimersByTimeAsync(2000);
    const ws4 = MockWebSocket.instances[3];
    ws4.emitOpen();
    expect(channel).toEqual([true, false, true]);
    expect(JSON.parse(ws4.sent[0]).type).toBe('subscribe');
    h.close();
  });

  it('stream_info 拉取失败 → 退避重试,不抛未捕获异常', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new Error('agentd 不可达');
      }),
    );
    const channel: boolean[] = [];
    openViewportStream({
      width: 100,
      height: 100,
      onFrame: () => {},
      onChannel: (up) => channel.push(up),
    });
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(500);
    await vi.advanceTimersByTimeAsync(1000);
    // 通道从未 up 过 → 不发多余的 down 回调(调用方本就从轮询腿起步);
    // 断言点:重试静默进行、无未捕获异常、WS 从未被构造。
    expect(channel).toEqual([]);
    expect(MockWebSocket.instances).toHaveLength(0);
  });
});
