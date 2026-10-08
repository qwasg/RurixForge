import { Fragment, useEffect, useRef, useState } from 'react';
import { ArrowDown, Shrink } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { activeEngineOf } from '@/lib/systemStore';
import { HOME_COL_MAX, type ChatVariant } from '@/lib/chatVariant';
import UserMessageCard from './UserMessageCard';
import AssistantMessage, { PendingAssistantMessage } from './AssistantMessage';
import { useCollaborationStore } from '@/lib/collaborationStore';
import ForgeMark from '@/components/ForgeMark';

/**
 * F7 wave.4 消息流(参考 render_chat_body):gap 14 / pt16 px16 pb8 纵滚;
 * 空态:无会话「选择左侧会话或点击 New Agent」,有会话无消息参考文案;
 * 滚动吸底(距底 <80 自动吸附,>160 出 ↓ 跳转钮)。
 * home 变体(全屏对话主页):滚动区铺满整屏,气泡收在 HOME_COL_MAX 居中列内,
 * 免得宽屏下一行拉到 1400px 没法读。
 *
 * D-047:①已发出、助手卡还没建(等 agent.started)时,末尾挂开轮前占位卡(同款头 + 轮次状态行);
 * ②吸底改由内容列 ResizeObserver 兜底——轮次状态行按计时出现 / 消失、代码块高亮完成等
 * 不经 messages 变化的增高,吸底态下同样跟到底。
 */
export default function MessageList({ variant = 'column' }: { variant?: ChatVariant }) {
  const messages = useChatStore((st) => st.messages);
  const compactions = useChatStore((st) => st.compactions);
  const hydrating = useChatStore((st) => st.hydrating);
  const pendingTurnSince = useChatStore((st) => st.pendingTurnSince);
  const activeSessionId = useSessionStore((st) => st.activeSessionId);
  const engine = useSessionStore(activeEngineOf);
  const agents = useCollaborationStore((state) => state.agents);

  const scrollRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);
  const [showJump, setShowJump] = useState(false);

  useEffect(() => {
    const el = scrollRef.current;
    if (el && pinnedRef.current) el.scrollTop = el.scrollHeight;
  }, [messages, hydrating, activeSessionId, pendingTurnSince]);

  useEffect(() => {
    const el = scrollRef.current;
    const content = contentRef.current;
    if (!el || !content || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => {
      if (pinnedRef.current) el.scrollTop = el.scrollHeight;
    });
    ro.observe(content);
    return () => ro.disconnect();
  }, []);

  // 切会话回吸底
  useEffect(() => {
    pinnedRef.current = true;
    setShowJump(false);
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [activeSessionId]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const dist = el.scrollHeight - el.scrollTop - el.clientHeight;
    pinnedRef.current = dist < 80;
    setShowJump(dist > 160);
  };

  const jumpToBottom = () => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    pinnedRef.current = true;
    setShowJump(false);
  };

  const home = variant === 'home';
  const visible = messages.filter((message) => !message.agentId || !agents.some((agent) => agent.id === message.agentId && agent.role !== 'root'));
  // 开轮前占位:只在末条仍是用户卡时挂(真卡一建出来就原位接替;发送失败 / 开轮前即收束已由 store 清掉)
  const awaitingSince = visible[visible.length - 1]?.role === 'user' ? pendingTurnSince : null;

  return (
    <div className="relative isolate flex min-h-0 flex-1 flex-col">
      {!home && !hydrating && messages.length === 0 && (
        <div data-testid="chat-empty-watermark" aria-hidden="true" className="pointer-events-none absolute inset-0 -z-10 flex items-center justify-center overflow-hidden">
          <ForgeMark variant="folded" className="w-[46%] max-w-[200px] select-none text-fg opacity-[0.035]" />
        </div>
      )}
      <div
        ref={scrollRef}
        onScroll={onScroll}
        data-testid="message-list"
        className={cn(
          'flex min-h-0 flex-1 flex-col overflow-y-auto pb-2 pt-4',
          home ? 'px-6' : 'px-4',
        )}
      >
        {/* 内容列:吸底的 ResizeObserver 观察它的高度(滚动容器自身尺寸不随内容变) */}
        <div
          ref={contentRef}
          data-testid="message-list-content"
          className={cn('flex w-full flex-col gap-[14px]', home && 'mx-auto')}
          style={home ? { maxWidth: HOME_COL_MAX } : undefined}
        >
          {!activeSessionId && (
            <p className="p-6 text-center text-[12px] text-fg-4">选择左侧会话或点击 New Agent</p>
          )}
          {activeSessionId && !hydrating && messages.length === 0 && (
            <p className="p-6 text-center text-[12px] text-fg-3">
              尚无任何消息。选择左侧会话后发送，或先点击「新建」创建会话。
            </p>
          )}
          {visible.map((m) => (
            <Fragment key={m.id}>
              {m.role === 'user' ? <UserMessageCard msg={m} /> : <AssistantMessage msg={m} />}
              {compactions.some((c) => c.afterMessageId === m.id) && <CompactedDivider />}
            </Fragment>
          ))}
          {awaitingSince !== null && <PendingAssistantMessage since={awaitingSince} engine={engine} />}
        </div>
      </div>
      {showJump && (
        <button
          type="button"
          aria-label="回到底部"
          data-testid="jump-bottom"
          onClick={jumpToBottom}
          className="absolute bottom-3 right-4 flex h-[26px] w-[26px] items-center justify-center rounded-full border border-edge bg-shell-float text-fg-3 shadow-float hover:text-fg"
        >
          <ArrowDown size={13} />
        </button>
      )}
    </div>
  );
}

/** 上下文压缩分隔线:之上的对话已总结成摘要,模型之后只看摘要 + 分隔线以下的新内容。 */
function CompactedDivider() {
  return (
    <div
      role="separator"
      data-testid="context-compacted-divider"
      className="flex items-center gap-3 py-1 text-[12px] text-fg-3"
    >
      <span className="h-px flex-1 bg-edge" />
      <span className="flex shrink-0 items-center gap-1.5">
        <Shrink size={12} />
        上下文已压缩，以上对话已总结为摘要
      </span>
      <span className="h-px flex-1 bg-edge" />
    </div>
  );
}
