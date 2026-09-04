import { useState } from 'react';
import { ArrowUp, BellRing, X } from 'lucide-react';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import { useToastStore } from '@/lib/toastStore';
import { editInputHeight } from '@/lib/inputHeight';
import { cn } from '@/lib/cn';
import { COMPOSER_MODES } from './composerModes';

/**
 * F7 wave.4 用户消息卡(参考 render_user_message):通栏卡 bg_panel 圆角 12 + line 边
 * + sh1(hover 边 accent_ring);头 HH:MM;正文 13px;底条 mode chip(20px 胶囊 bg_sunk)。
 * 点击进内联编辑(56–200 自适应 textarea +「编辑后重新发送,将回退此后的对话」提示
 * + 24px 圆 x 取消 + accent 圆 arrow-up 重发 = revert before + resend)。
 *
 * D-038:`msg.source === 'receipt'` 是系统唤醒轮——正文是系统生成的「回执已送达」说明,
 * 不是用户说的话:虚线边 + 「回执唤醒」chip,不提供编辑重发(重发一段系统话没有意义,
 * 而且 revert 会把已送达的回执卡片一起截掉)。
 */
export default function UserMessageCard({ msg }: { msg: ChatMsg }) {
  const activeRunId = useChatStore((st) => st.activeRunId);
  const editAndResend = useChatStore((st) => st.editAndResend);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');

  const modeMeta = COMPOSER_MODES.find((m) => m.id === msg.mode);
  const isWake = msg.source === 'receipt';
  const startEdit = () => {
    if (isWake) return;
    if (activeRunId) {
      useToastStore.getState().push('error', '当前有任务运行中，请先等待完成或中止');
      return;
    }
    setDraft(msg.text);
    setEditing(true);
  };

  const resend = () => {
    const text = draft.trim();
    if (text === '') {
      useToastStore.getState().push('error', '请输入新的消息内容');
      return;
    }
    setEditing(false);
    void editAndResend(msg.id, text);
  };

  return (
    <div
      data-testid="user-message-card"
      data-source={msg.source ?? 'user'}
      className={cn(
        'relative flex flex-col gap-1.5 rounded-xl border px-3 pb-2 pt-2.5 shadow-sh1',
        isWake
          ? 'border-dashed border-edge bg-shell-sunk'
          : editing
            ? 'border-acc-ring bg-shell-panel'
            : 'border-edge bg-shell-panel hover:border-acc-ring',
      )}
    >
      {/* 头:HH:MM + 编辑提示 */}
      {(msg.time !== '' || editing) && (
        <div className="flex items-center gap-1.5 text-[11px] text-fg-3">
          {msg.time !== '' && <span>{msg.time}</span>}
          <span className="flex-1" />
          {editing && <span>编辑后重新发送，将回退此后的对话</span>}
        </div>
      )}
      {/* 正文 / 内联编辑 */}
      {editing ? (
        <textarea
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          data-testid="user-edit-input"
          className="w-full resize-none bg-transparent text-[13.5px] text-fg outline-none"
          style={{ height: editInputHeight(draft), minHeight: 56 }}
        />
      ) : (
        <div
          className={cn('whitespace-pre-wrap text-[13px]', isWake ? 'text-fg-2' : 'cursor-pointer text-fg')}
          data-testid="user-message-text"
          title={isWake ? undefined : '点击编辑并重发'}
          onClick={startEdit}
        >
          {msg.text}
        </div>
      )}
      {/* 底条:mode chip + 动作 */}
      <div className="flex items-center gap-1.5 pt-0.5">
        {isWake && (
          <span
            data-testid="user-wake-chip"
            className="flex h-5 items-center gap-1 rounded-full bg-acc-bg px-2 text-[11px] text-acc"
          >
            <BellRing size={11} />
            回执唤醒
          </span>
        )}
        <span className="flex h-5 items-center gap-1 rounded-full bg-shell-sunk px-2 text-[11px] text-fg-3">
          {modeMeta && <modeMeta.icon size={11} />}
          {modeMeta?.label ?? 'Agent'}
        </span>
        <span className="flex-1" />
        {isWake ? null : editing ? (
          <>
            <button
              type="button"
              aria-label="取消编辑"
              data-testid="user-edit-cancel"
              onClick={() => setEditing(false)}
              className="flex h-6 w-6 items-center justify-center rounded-full border border-edge text-fg-3 hover:bg-shell-hover"
            >
              <X size={11} />
            </button>
            <button
              type="button"
              aria-label="重新发送"
              data-testid="user-edit-resend"
              onClick={resend}
              className="flex h-6 w-6 items-center justify-center rounded-full bg-acc text-fg-inv hover:bg-acc-soft"
            >
              <ArrowUp size={12} />
            </button>
          </>
        ) : (
          <button
            type="button"
            aria-label="编辑并重发"
            data-testid="user-edit-enter"
            onClick={startEdit}
            className="flex h-6 w-6 items-center justify-center rounded-full bg-shell-active text-fg-3 hover:bg-shell-hover"
          >
            <ArrowUp size={12} />
          </button>
        )}
      </div>
    </div>
  );
}
