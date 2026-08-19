import { useEffect, useRef, useState } from 'react';
import { ArrowDown } from 'lucide-react';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import UserMessageCard from './UserMessageCard';
import AssistantMessage from './AssistantMessage';

/**
 * F7 wave.4 消息流(参考 render_chat_body):gap 14 / pt16 px16 pb8 纵滚;
 * 空态:无会话「选择左侧会话或点击 New Agent」,有会话无消息参考文案;
 * 滚动吸底(距底 <80 自动吸附,>160 出 ↓ 跳转钮)。
 */
export default function MessageList() {
  const messages = useChatStore((st) => st.messages);
  const hydrating = useChatStore((st) => st.hydrating);
  const activeSessionId = useSessionStore((st) => st.activeSessionId);

  const scrollRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);
  const [showJump, setShowJump] = useState(false);

  useEffect(() => {
    const el = scrollRef.current;
    if (el && pinnedRef.current) el.scrollTop = el.scrollHeight;
  }, [messages, hydrating, activeSessionId]);

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

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <div
        ref={scrollRef}
        onScroll={onScroll}
        data-testid="message-list"
        className="flex min-h-0 flex-1 flex-col gap-[14px] overflow-y-auto px-4 pb-2 pt-4"
      >
        {!activeSessionId && (
          <p className="p-6 text-center text-[12px] text-fg-4">选择左侧会话或点击 New Agent</p>
        )}
        {activeSessionId && !hydrating && messages.length === 0 && (
          <p className="p-6 text-center text-[12px] text-fg-3">
            尚无任何消息。选择左侧会话后发送，或先点击「新建」创建会话。
          </p>
        )}
        {messages.map((m) =>
          m.role === 'user' ? (
            <UserMessageCard key={m.id} msg={m} />
          ) : (
            <AssistantMessage key={m.id} msg={m} />
          ),
        )}
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
