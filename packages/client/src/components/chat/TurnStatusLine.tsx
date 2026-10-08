import { useEffect, useRef, useState } from 'react';
import { cn } from '@/lib/cn';
import { useChatStore } from '@/lib/chatStore';
import { useSystemStore } from '@/lib/systemStore';
import type { ChatBlock } from '@/lib/timeline';
import { deriveTurnStatus } from '@/lib/turnStatus';
import StreamEnter from './StreamEnter';

/** 状态行重算节拍:去抖窗口最短 400ms,秒数按秒跳,半秒一拍足够。 */
const TICK_MS = 500;

/** 只为驱动重渲染的节拍(时刻在渲染里现取,免得拍子与块变化的先后算出负空档)。 */
function useTick(ms: number): void {
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((n) => n + 1), ms);
    return () => clearInterval(id);
  }, [ms]);
}

/** 浏览器联网态(online / offline 事件)。 */
function useNetworkOnline(): boolean {
  const [online, setOnline] = useState(() => typeof navigator === 'undefined' || navigator.onLine !== false);
  useEffect(() => {
    const up = () => setOnline(true);
    const down = () => setOnline(false);
    window.addEventListener('online', up);
    window.addEventListener('offline', down);
    return () => {
      window.removeEventListener('online', up);
      window.removeEventListener('offline', down);
    };
  }, []);
  return online;
}

/**
 * 最后一次可见进展的时刻:chatStore 每次改动这条消息都会换一份 blocks 数组(mutateAssistant 克隆),
 * 于是 blocks 身份变了 = 本轮有新动静。在渲染里同步记,不等 effect——否则块刚变的那一帧
 * 还拿旧时刻算,会闪一帧「Planning」。首帧取 origin(占位卡传发出时刻;真卡取挂载时刻)。
 */
function useLastChange(blocks: ChatBlock[], origin: number | undefined): number {
  const ref = useRef<{ blocks: ChatBlock[]; at: number } | null>(null);
  if (ref.current === null) ref.current = { blocks, at: origin ?? Date.now() };
  else if (ref.current.blocks !== blocks) ref.current = { blocks, at: Date.now() };
  return ref.current.at;
}

/**
 * D-047 轮次状态行(状态机见 lib/turnStatus.ts):挂在流式助手卡末尾(取代原 accent 闪烁 caret),
 * 以及「已发出、助手卡还没建」的占位卡里。有行正在执行时不出(那一行自己扫光)。
 * 形态与过程链同列:12.5px 两段式;执行中整行扫光(.forge-shimmer),等用户 / 断线静止 text_2。
 * 换状态时按 kind 重挂,新文案自上而下入场;秒数跳动不重挂,且不进读屏播报。
 */
export default function TurnStatusLine({ blocks, since }: { blocks: ChatBlock[]; since?: number }) {
  useTick(TICK_MS);
  const lastChange = useLastChange(blocks, since);
  const streamLink = useChatStore((st) => st.streamLink);
  const downSince = useChatStore((st) => st.streamDownSince);
  const hostOffline = useSystemStore((st) => st.checked && !st.online);
  const networkOnline = useNetworkOnline();

  const now = Date.now();
  const down = streamLink === 'down';
  const status = deriveTurnStatus({
    blocks,
    idleMs: Math.max(0, now - lastChange),
    link: {
      down,
      downForMs: down && downSince !== null ? Math.max(0, now - downSince) : 0,
      hostOffline,
      networkOffline: !networkOnline,
    },
  });
  if (!status) return null;

  return (
    <div data-testid="turn-status" data-state={status.kind} role="status" className="flex py-0.5 text-[12.5px]">
      <StreamEnter key={status.kind} active className="flex min-w-0">
        <span
          className={cn(
            'inline-flex min-w-0 items-center gap-[5px]',
            status.live ? 'forge-shimmer' : 'text-fg-2',
          )}
        >
          {/* 带补充时文案后缀一个空格(同 SummaryLine:行尾空白不渲染,视距由 gap 定;复制 / 读屏不黏连);
              补充以「·」隔开(同工具详情「Done · 34ms」),整句短语后直接跟秒数 / 原因读着黏 */}
          <span className="truncate">
            {status.label}
            {status.detail !== '' && ' '}
          </span>
          {status.detail !== '' && (
            <span aria-hidden={status.kind === 'slow'} className={cn('shrink-0', !status.live && 'text-fg-3')}>
              · {status.detail}
            </span>
          )}
        </span>
      </StreamEnter>
    </div>
  );
}
