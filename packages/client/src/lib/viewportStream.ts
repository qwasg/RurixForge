/**
 * viewportStream:视口直连推流通道(WS 帧推送 + 实时输入)。
 *
 * 既有帧通路(100ms 轮询 → host 代理 → agentd → MCP stdio → TCP → base64 JSON 原路
 * 返回)实测 5-8 fps;本模块经 `viewport_stream_info` 拿到 engine-host 的 WS 直连
 * 地址后,浏览器直收**二进制 RGBA8 帧**(20B 头 + 紧凑像素),并把键盘/相机/选中/
 * 尺寸消息经同一连接低延迟回传(play 态游戏可实时游玩)。
 *
 * 帧消息布局(小端):`magic "FGF1" | frameId u32 | width u16 | height u16 |
 * flags u32 | draws u32`;flags bit0=play_running bit1=truncated bit2=imported。
 *
 * 通道状态经 onUp/onDown 如实回调:连不上/断流由 ViewportCanvas 回退 MCP 轮询腿
 * (诚实降级,不黑屏);本模块带 capped 指数退避自动重连,恢复后回调 onUp。
 */

import { callTool } from './forgeApi';

export const FRAME_HEADER_LEN = 20;
const FRAME_MAGIC = 0x31464746; // "FGF1" 小端 u32

/** 一帧解析结果(rgba 为消息 buffer 上的视图,零拷贝;绘制方即用即弃) */
export interface StreamFrame {
  frameId: number;
  width: number;
  height: number;
  playing: boolean;
  truncated: boolean;
  imported: boolean;
  draws: number;
  rgba: Uint8ClampedArray;
}

/** 服务端 1Hz 状态文本 */
export interface StreamStatus {
  playState: string;
  deviceName: string;
  draws: number;
  truncated: boolean;
  fps: number;
  shareError?: string;
}

export interface ViewportStreamOptions {
  width: number;
  height: number;
  maxFps?: number;
  /** 二进制帧到达(已解析;latest-wins 由调用方 rAF 侧自然形成) */
  onFrame: (f: StreamFrame) => void;
  /** 服务端 1Hz 状态 */
  onStatus?: (s: StreamStatus) => void;
  /** 服务端渲染降级(DEV_ENV_DEGRADE 等,如实上屏) */
  onError?: (message: string) => void;
  /** 通道可用性变化:up=false 时调用方应回退轮询腿 */
  onChannel?: (up: boolean, reason: string) => void;
}

export interface ViewportStreamHandle {
  /** play 态实时输入。true 表示当前 socket 已接受发送，不等于引擎执行成功；false 表示未发送。 */
  sendInput(action: string, value: number): boolean;
  /**
   * play 态指针点击:x/y 为归一化视口坐标(0..1,左上原点)。引擎按游戏相机反投影到游戏平面,
   * 依次派发 `<action>_x/_y/_z`(世界坐标)与 `<action>`(value 1)——图侧可按格子/实体位置响应。
   */
  sendPointer(action: string, x: number, y: number): boolean;
  /** 编辑器相机绝对量(与 viewport_set_camera 同字段;服务端同套钳制) */
  sendCamera(cam: Record<string, unknown>): void;
  /** 选中高亮(null 清除) */
  setSelected(id: number | null): void;
  /** 流分辨率协商(客户端已去抖) */
  resize(width: number, height: number): void;
  /** 通道当前是否可用 */
  readonly up: boolean;
  close(): void;
}

/** 解析二进制帧消息;坏头(魔数/长度不符)返回 null,由调用方计入通道异常。 */
export function parseFrameMessage(buf: ArrayBuffer): StreamFrame | null {
  if (buf.byteLength < FRAME_HEADER_LEN) return null;
  const dv = new DataView(buf);
  if (dv.getUint32(0, true) !== FRAME_MAGIC) return null;
  const frameId = dv.getUint32(4, true);
  const width = dv.getUint16(8, true);
  const height = dv.getUint16(10, true);
  const flags = dv.getUint32(12, true);
  const draws = dv.getUint32(16, true);
  if (buf.byteLength !== FRAME_HEADER_LEN + width * height * 4) return null;
  return {
    frameId,
    width,
    height,
    playing: (flags & 1) !== 0,
    truncated: (flags & 2) !== 0,
    imported: (flags & 4) !== 0,
    draws,
    rgba: new Uint8ClampedArray(buf, FRAME_HEADER_LEN),
  };
}

/** play 态键盘 → 引擎输入契约:action = 语义键名,value 符号编码轴向
 * (左/下 = -1,右/上 = +1;keyup 发同 action value 0)。
 * 与现有游戏图核对过:breakout 按 value 符号判向、PvZ 任意正值触发、maze 按 action 名分支。 */
export function mapKeyToInput(key: string): { action: string; value: number } | null {
  switch (key) {
    case 'ArrowLeft':
    case 'a':
    case 'A':
      return { action: 'left', value: -1 };
    case 'ArrowRight':
    case 'd':
    case 'D':
      return { action: 'right', value: 1 };
    case 'ArrowUp':
    case 'w':
    case 'W':
      return { action: 'up', value: 1 };
    case 'ArrowDown':
    case 's':
    case 'S':
      return { action: 'down', value: -1 };
    case ' ':
      return { action: 'space', value: 1 };
    default:
      return null;
  }
}

/** 重连退避(ms):断线立即回调 onChannel(false),后台最多按 5s 间隔续试。 */
const BACKOFF_MS = [500, 1000, 2000, 5000];

interface StreamInfoResp {
  wsUrl: string;
  proto: number;
}

/**
 * 打开视口推流通道。输入可能是购买/施法等一次性命令，断线时返回 false，
 * 不缓存或重放。调用方应提示重新操作；选中与分辨率配置会随订阅恢复。
 */
export function openViewportStream(opts: ViewportStreamOptions): ViewportStreamHandle {
  let ws: WebSocket | null = null;
  let closed = false;
  let up = false;
  let fails = 0;
  let retryTimer: number | undefined;
  let cfg = { width: opts.width, height: opts.height, maxFps: opts.maxFps ?? 60 };
  let selected: number | null = null;

  const setUp = (next: boolean, reason: string) => {
    if (up === next) return;
    up = next;
    opts.onChannel?.(next, reason);
  };

  const sendJson = (msg: Record<string, unknown>): boolean => {
    const sock = ws;
    if (closed || !sock || sock.readyState !== WebSocket.OPEN) {
      setUp(false, '连接不可用');
      return false;
    }
    try {
      sock.send(JSON.stringify(msg));
      return true;
    } catch {
      setUp(false, '发送失败');
      // 由 onclose 安排重连，失败的一次性输入不自动补发。
      sock.close();
      return false;
    }
  };

  const scheduleRetry = (reason: string) => {
    if (closed) return;
    fails += 1;
    setUp(false, reason);
    const wait = BACKOFF_MS[Math.min(fails - 1, BACKOFF_MS.length - 1)];
    retryTimer = window.setTimeout(() => void connect(), wait);
  };

  const connect = async () => {
    if (closed) return;
    let info: StreamInfoResp;
    try {
      info = await callTool<StreamInfoResp>('viewport_stream_info');
    } catch (err) {
      scheduleRetry(`stream_info 失败: ${(err as Error).message}`);
      return;
    }
    if (closed) return;
    let sock: WebSocket;
    try {
      sock = new WebSocket(info.wsUrl);
    } catch (err) {
      scheduleRetry(`WS 构造失败: ${(err as Error).message}`);
      return;
    }
    sock.binaryType = 'arraybuffer';
    ws = sock;
    sock.onopen = () => {
      if (closed || ws !== sock) return;
      if (!sendJson({ type: 'subscribe', ...cfg })) return;
      // 订阅即带上当前选中(高亮跨重连保持)。
      if (selected != null && !sendJson({ type: 'select', id: selected })) return;
      fails = 0;
      setUp(true, 'connected');
    };
    sock.onmessage = (ev: MessageEvent) => {
      if (closed || ws !== sock) return;
      if (ev.data instanceof ArrayBuffer) {
        const f = parseFrameMessage(ev.data);
        if (f) opts.onFrame(f);
        return;
      }
      if (typeof ev.data === 'string') {
        try {
          const v = JSON.parse(ev.data) as Record<string, unknown> & { type?: string };
          if (v.type === 'status') opts.onStatus?.(v as unknown as StreamStatus);
          else if (v.type === 'error') opts.onError?.(String(v.message ?? '未知服务端错误'));
        } catch {
          // 坏文本静默(协议外噪声)
        }
      }
    };
    sock.onclose = () => {
      if (closed || ws !== sock) return;
      ws = null;
      setUp(false, '连接断开');
      scheduleRetry('连接断开');
    };
    // onerror 后必有 onclose,统一在 onclose 走重连,避免双计失败。
    sock.onerror = () => {};
  };

  void connect();

  return {
    get up() {
      return up && !closed && ws?.readyState === WebSocket.OPEN;
    },
    sendInput(action, value) {
      return sendJson({ type: 'input', action, value });
    },
    sendPointer(action, x, y) {
      return sendJson({ type: 'pointer', action, x, y });
    },
    sendCamera(cam) {
      sendJson({ type: 'camera', ...cam });
    },
    setSelected(id) {
      selected = id;
      sendJson({ type: 'select', ...(id != null ? { id } : {}) });
    },
    resize(width, height) {
      cfg = { ...cfg, width, height };
      sendJson({ type: 'resize', width, height });
    },
    close() {
      closed = true;
      setUp(false, 'closed');
      window.clearTimeout(retryTimer);
      const sock = ws;
      ws = null;
      if (sock) {
        sock.onclose = null;
        sock.onmessage = null;
        try {
          sock.close();
        } catch {
          // 已断开
        }
      }
    },
  };
}
