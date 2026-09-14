/**
 * F7 wave.4 自研 SSE 客户端(G-F7-4;参考 api/sse.rs 对端语义)。
 * 不用 EventSource:不可控 fromSeq 续传/自定义重连,故 fetch + ReadableStream 逐帧解析。
 *
 * - subscribe(sessionId, fromSeq, cb) → { close() };帧格式 id=seq / event=type / data=wire JSON。
 * - 指数退避 500ms×1.7 封顶 10s 自动重连;重连带最新 seq(fromSeq=lastSeq)续传。
 * - 收到 event=stream.gap 帧 → onGap(调用方重拉 design-snapshot 再以最新 latestSeq 重订阅)。
 * - close() 经 AbortController 断流,幂等;close 后不再重连/回调。
 */

/** 解析后的一帧 SSE(data 已 JSON.parse;解析失败给 null 如实跳过由调用方判)。 */
export interface SseFrame {
  /** 帧 id(agentd 侧 = seq 十进制串;gap 帧无 id)。 */
  id: string | null;
  /** event 字段(= 事件 type;keep-alive 注释帧不会到这里)。 */
  event: string;
  /** data JSON 解析结果(非 JSON 时为 null)。 */
  data: unknown;
}

export interface SseCallbacks {
  onEvent: (frame: SseFrame) => void;
  /** stream.gap:replay-window-exceeded / subscriber-lagged。 */
  onGap?: (reason: string) => void;
  /** 网络/HTTP 错误(重连前触发;close 后不再触发)。 */
  onError?: (err: unknown) => void;
  /** 每次连接成功打开(含重连)时触发。 */
  onOpen?: () => void;
}

export interface SseSubscription {
  close: () => void;
  /** 当前已见最大 seq(重连续传用;测试可观测)。 */
  lastSeq: () => number;
}

export const SSE_BACKOFF_BASE_MS = 500;
export const SSE_BACKOFF_FACTOR = 1.7;
export const SSE_BACKOFF_CAP_MS = 10_000;

/** 退避序列:500, 850, 1445, … 封顶 10000(纯函数抽出供单测)。 */
export function backoffDelay(attempt: number): number {
  const d = SSE_BACKOFF_BASE_MS * Math.pow(SSE_BACKOFF_FACTOR, attempt);
  return Math.min(Math.round(d), SSE_BACKOFF_CAP_MS);
}

/** SSE 帧切分(黏包/跨块):\n\n 或 \r\n\r\n 分隔;返回 [完整帧, 残余]。 */
export function splitFrames(buf: string): [string[], string] {
  const frames: string[] = [];
  let rest = buf;
  for (;;) {
    const m = rest.match(/\r?\n\r?\n/);
    if (!m || m.index === undefined) break;
    frames.push(rest.slice(0, m.index));
    rest = rest.slice(m.index + m[0].length);
  }
  return [frames, rest];
}

/** 单帧文本 → SseFrame(注释行/空帧 → null;data 多行 \n 拼接)。 */
export function parseFrame(raw: string): SseFrame | null {
  let id: string | null = null;
  let event = '';
  const dataLines: string[] = [];
  for (const line of raw.split(/\r?\n/)) {
    if (line.startsWith(':')) continue; // keep-alive 注释帧
    if (line.startsWith('id:')) id = line.slice(3).replace(/^ /, '');
    else if (line.startsWith('event:')) event = line.slice(6).replace(/^ /, '');
    else if (line.startsWith('data:')) dataLines.push(line.slice(5).replace(/^ /, ''));
  }
  if (event === '' && dataLines.length === 0) return null;
  let data: unknown = null;
  const text = dataLines.join('\n');
  if (text !== '') {
    try {
      data = JSON.parse(text);
    } catch {
      data = null; // 非 JSON data 如实 null(调用方按 event 名判)
    }
  }
  return { id, event, data };
}

/**
 * 订阅会话事件流。fetch 失败/流中断均按退避重连(带最新 seq);
 * gap 帧只回调不自杀(调用方决定重拉+重订阅)。
 */
export function subscribeSessionEvents(
  sessionId: string,
  fromSeq: number,
  cb: SseCallbacks,
): SseSubscription {
  let closed = false;
  let seq = fromSeq;
  let attempt = 0;
  let abort: AbortController | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const close = () => {
    if (closed) return; // 幂等
    closed = true;
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    abort?.abort();
    abort = null;
  };

  const scheduleRetry = () => {
    if (closed) return;
    const delay = backoffDelay(attempt);
    attempt += 1;
    timer = setTimeout(() => {
      timer = null;
      void connect();
    }, delay);
  };

  const connect = async () => {
    if (closed) return;
    abort = new AbortController();
    let res: Response;
    try {
      res = await fetch(
        `/api/forge/sessions/${encodeURIComponent(sessionId)}/events/stream?fromSeq=${seq}`,
        { signal: abort.signal, headers: { Accept: 'text/event-stream' } },
      );
    } catch (err) {
      if (closed) return; // close 触发的 abort 不算错误
      cb.onError?.(err);
      scheduleRetry();
      return;
    }
    if (!res.ok || !res.body) {
      if (closed) return;
      cb.onError?.(new Error(`SSE HTTP ${res.status}`));
      scheduleRetry();
      return;
    }
    attempt = 0; // 连接成功 → 退避复位
    cb.onOpen?.();
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buf = '';
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (closed) return;
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        const [frames, rest] = splitFrames(buf);
        buf = rest;
        for (const raw of frames) {
          const frame = parseFrame(raw);
          if (!frame) continue;
          if (frame.event === 'stream.gap') {
            const reason =
              (frame.data as { payload?: { reason?: unknown } } | null)?.payload?.reason;
            cb.onGap?.(typeof reason === 'string' ? reason : 'unknown');
            continue;
          }
          // id = seq(十进制串):推进续传游标
          if (frame.id !== null) {
            const n = Number(frame.id);
            if (Number.isFinite(n) && n > seq) seq = n;
          }
          cb.onEvent(frame);
        }
      }
    } catch (err) {
      if (closed) return;
      cb.onError?.(err);
    }
    if (!closed) scheduleRetry(); // 流正常结束(对端关闭)也重连
  };

  void connect();
  return { close, lastSeq: () => seq };
}
