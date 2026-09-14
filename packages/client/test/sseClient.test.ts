import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  backoffDelay,
  parseFrame,
  splitFrames,
  subscribeSessionEvents,
} from '@/lib/sseClient';

/** 手工 ReadableStream:按 chunk 逐段 enqueue 后 close(黏包/分块场景构造)。 */
function streamOf(chunks: string[]): ReadableStream<Uint8Array> {
  const enc = new TextEncoder();
  return new ReadableStream({
    start(controller) {
      for (const c of chunks) controller.enqueue(enc.encode(c));
      controller.close();
    },
  });
}

function sseResponse(chunks: string[]): Response {
  return { ok: true, status: 200, body: streamOf(chunks) } as unknown as Response;
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe('sseClient 帧解析(纯函数)', () => {
  it('多帧一次到达:逐帧 id/event/data 解析', () => {
    const text =
      'id: 1\nevent: agent.started\ndata: {"seq":1,"type":"agent.started"}\n\n' +
      'id: 2\nevent: agent.message\ndata: {"seq":2,"type":"agent.message"}\n\n';
    const [frames, rest] = splitFrames(text);
    expect(rest).toBe('');
    expect(frames).toHaveLength(2);
    const f0 = parseFrame(frames[0]);
    const f1 = parseFrame(frames[1]);
    expect(f0).toMatchObject({ id: '1', event: 'agent.started' });
    expect((f0?.data as { seq: number }).seq).toBe(1);
    expect(f1).toMatchObject({ id: '2', event: 'agent.message' });
  });

  it('分块黏包:跨块帧在残余中等待拼齐', () => {
    const a = 'id: 1\nevent: foo\ndata: {"a":';
    const [f1, rest1] = splitFrames(a);
    expect(f1).toHaveLength(0);
    const [f2, rest2] = splitFrames(rest1 + '1}\n\nid: 2\nevent: bar\ndata: {}\n\n');
    expect(rest2).toBe('');
    expect(f2).toHaveLength(2);
    expect(parseFrame(f2[0])?.data).toEqual({ a: 1 });
  });

  it('keep-alive 注释帧与 CRLF 处理', () => {
    const [frames] = splitFrames(': keep-alive\r\n\r\nid: 5\r\nevent: x\r\ndata: {"k":1}\r\n\r\n');
    expect(frames).toHaveLength(2);
    expect(parseFrame(frames[0])).toBeNull();
    expect(parseFrame(frames[1])).toMatchObject({ id: '5', event: 'x' });
  });

  it('退避序列:500×1.7 封顶 10000', () => {
    expect(backoffDelay(0)).toBe(500);
    expect(backoffDelay(1)).toBe(850);
    expect(backoffDelay(2)).toBe(1445);
    expect(backoffDelay(100)).toBe(10_000);
  });
});

describe('sseClient 订阅(fetch mock)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  it('正常流:事件回调 + lastSeq 推进 + 流结束后带最新 seq 重连', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(sseResponse(['id: 3\nevent: agent.started\ndata: {"seq":3}\n\n']))
      .mockResolvedValueOnce(sseResponse(['id: 4\nevent: agent.completed\ndata: {"seq":4}\n\n']))
      .mockImplementation(() => new Promise(() => {})); // 第三次起挂起
    vi.stubGlobal('fetch', fetchMock);
    const events: unknown[] = [];
    const sub = subscribeSessionEvents('sess_1', 0, { onEvent: (f) => events.push(f) });
    await vi.advanceTimersByTimeAsync(0);
    expect(events).toHaveLength(1);
    expect(sub.lastSeq()).toBe(3);
    // 首流正常结束 → 立即重连(attempt 已复位,delay=500)
    await vi.advanceTimersByTimeAsync(500);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(String(fetchMock.mock.calls[1][0])).toContain('fromSeq=3');
    await vi.advanceTimersByTimeAsync(0);
    expect(events).toHaveLength(2);
    expect(sub.lastSeq()).toBe(4);
    await vi.advanceTimersByTimeAsync(500);
    expect(String(fetchMock.mock.calls[2][0])).toContain('fromSeq=4');
    sub.close();
  });

  it('错误退避:fetch 拒绝 → 500 → 850 重连;onError 回调', async () => {
    const fetchMock = vi
      .fn()
      .mockRejectedValueOnce(new Error('net down'))
      .mockRejectedValueOnce(new Error('net down'))
      .mockResolvedValueOnce(sseResponse(['id: 1\nevent: x\ndata: {}\n\n']));
    vi.stubGlobal('fetch', fetchMock);
    const errors: unknown[] = [];
    const sub = subscribeSessionEvents('sess_1', 0, {
      onEvent: () => {},
      onError: (e) => errors.push(e),
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(errors).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(499);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(849);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(errors).toHaveLength(2);
    sub.close();
  });

  it('gap 帧:event=stream.gap → onGap(reason),不进 onEvent、不推进 seq', async () => {
    const gap =
      'event: stream.gap\ndata: {"type":"stream.gap","payload":{"gap":true,"reason":"replay-window-exceeded"}}\n\n' +
      'id: 8\nevent: session.updated\ndata: {"seq":8}\n\n';
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(sseResponse([gap]))
      .mockImplementation(() => new Promise(() => {}));
    vi.stubGlobal('fetch', fetchMock);
    const events: unknown[] = [];
    const gaps: string[] = [];
    const sub = subscribeSessionEvents('sess_1', 5, {
      onEvent: (f) => events.push(f),
      onGap: (r) => gaps.push(r),
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(gaps).toEqual(['replay-window-exceeded']);
    expect(events).toHaveLength(1);
    expect(sub.lastSeq()).toBe(8);
    sub.close();
  });

  it('close 幂等:双调不报错;close 后不再重连不再回调', async () => {
    const fetchMock = vi.fn().mockRejectedValue(new Error('down'));
    vi.stubGlobal('fetch', fetchMock);
    const errors: unknown[] = [];
    const sub = subscribeSessionEvents('sess_1', 0, {
      onEvent: () => {},
      onError: (e) => errors.push(e),
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    sub.close();
    sub.close();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(fetchMock).toHaveBeenCalledTimes(1); // close 后不得重连
    expect(errors).toHaveLength(1);
  });

  it('HTTP 非 2xx 视为错误并重连', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: false, status: 404, body: null } as unknown as Response)
      .mockImplementation(() => new Promise(() => {}));
    vi.stubGlobal('fetch', fetchMock);
    const errors: unknown[] = [];
    const sub = subscribeSessionEvents('sess_1', 0, {
      onEvent: () => {},
      onError: (e) => errors.push(e),
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(errors).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(500);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    sub.close();
  });
});
