import { useEffect, useRef, useState } from 'react';
import { GitBranch, MoreHorizontal } from 'lucide-react';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { IBtn, MenuItem } from './primitives';
import { StatusDot } from './primitives';
import MessageList from '../chat/MessageList';
import Composer from '../chat/Composer';
import SubagentOverlay from '../chat/SubagentOverlay';

/**
 * F7 wave.4 对话列(参考 ui/chat.rs render_chat_column):
 * 38px 头(状态点/标题/fork/more[重命名/删除 真实接线])+
 * MessageList(SSE 驱动)+ Composer 全量 + SubagentOverlay(浮于 composer 上方)。
 * 会话切换 → chatStore.selectSession(快照回放 + SSE 订阅;切换关旧流)。
 */
export default function ChatColumn() {
  const activeSessionId = useSessionStore((st) => st.activeSessionId);
  const activeSession = useSessionStore((st) => st.sessions.find((s) => s.id === st.activeSessionId));
  const fork = useSessionStore((st) => st.fork);
  const remove = useSessionStore((st) => st.remove);
  const rename = useSessionStore((st) => st.rename);
  const activeRunId = useChatStore((st) => st.activeRunId);

  const [menuOpen, setMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState('');
  const headRef = useRef<HTMLDivElement>(null);

  // 会话切换 → 快照回放 + SSE 订阅(chatStore 内关旧流)
  useEffect(() => {
    void useChatStore.getState().selectSession(activeSessionId);
  }, [activeSessionId]);

  useEffect(() => {
    if (!menuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (headRef.current && !headRef.current.contains(e.target as Node)) setMenuOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [menuOpen]);

  const title = activeSession
    ? activeSession.title === ''
      ? activeSession.id
      : activeSession.title
    : '';

  const commitRename = () => {
    setRenaming(false);
    if (!activeSession) return;
    const t = draft.trim();
    if (t !== '' && t !== activeSession.title) void rename(activeSession.id, t);
  };

  return (
    <div data-testid="chat-column" className="relative flex h-full min-h-0 flex-col bg-shell-panel">
      {/* 38px 头 */}
      <div
        ref={headRef}
        className="relative flex h-[38px] shrink-0 items-center gap-1.5 border-b border-edge px-3"
      >
        {activeSession && (
          <StatusDot
            color={activeRunId ? 'var(--dot-running)' : 'var(--dot-done)'}
            pulse={activeRunId != null}
          />
        )}
        {renaming && activeSession ? (
          <input
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commitRename}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitRename();
              if (e.key === 'Escape') setRenaming(false);
            }}
            className="min-w-0 flex-1 rounded border border-acc-ring bg-shell-input px-1 py-px text-[13px] font-semibold text-fg outline-none"
          />
        ) : (
          <span className="min-w-0 flex-1 truncate text-[13px] font-semibold text-fg" data-testid="chat-head-title">
            {title}
          </span>
        )}
        {activeSession && (
          <>
            <IBtn
              title="分叉会话"
              testId="chat-fork"
              onClick={() => void fork(activeSession.id)}
            >
              <GitBranch size={13} />
            </IBtn>
            <IBtn title="更多" testId="chat-more" onClick={() => setMenuOpen((v) => !v)}>
              <MoreHorizontal size={13} />
            </IBtn>
          </>
        )}
        {menuOpen && activeSession && (
          <div
            role="menu"
            className="absolute right-2 top-[36px] z-40 min-w-[160px] rounded-lg border border-edge-strong bg-shell-float p-1 shadow-float"
          >
            <MenuItem
              label="重命名"
              onSelect={() => {
                setMenuOpen(false);
                setDraft(activeSession.title);
                setRenaming(true);
              }}
            />
            <MenuItem
              label="删除会话"
              onSelect={() => {
                setMenuOpen(false);
                void remove(activeSession.id);
              }}
            />
          </div>
        )}
      </div>

      {/* 消息流 + Composer + 子代理浮层 */}
      <MessageList />
      <Composer />
      <SubagentOverlay />
    </div>
  );
}
