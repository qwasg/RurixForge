import { useEffect, useRef, useState } from 'react';
import { ArrowUp, BellRing, Check, ChevronDown, Copy, X } from 'lucide-react';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import { copyText } from '@/lib/clipboard';
import { useSessionStore } from '@/lib/sessionStore';
import { useToastStore } from '@/lib/toastStore';
import { editInputHeight } from '@/lib/inputHeight';
import { cn } from '@/lib/cn';
import { COMPOSER_MODES, composerModeMeta, modesForKind } from './composerModes';
import { useFlowContext } from './ultraplan/flowContext';
import { MESSAGE_STATUS_LABEL, useCollaborationStore } from '@/lib/collaborationStore';
import type { AgentMessageStatus } from '@forge/protocol';
import AnnotationChips from './AnnotationChips';

const ULTRA_LOCK_TITLE = 'UltraPlan 流程内的消息不能回退,请用「重新开始」';

/**
 * F7 wave.4 用户消息卡(参考 render_user_message):通栏卡 bg_panel 圆角 12 + line 边
 * + sh1(hover 边 accent_ring);头 HH:MM;正文 13px;底条 mode chip(20px 胶囊 bg_sunk)。
 * 点击进内联编辑(56–200 自适应 textarea +「编辑后重新发送,将回退此后的对话」提示
 * + 24px 圆 x 取消 + accent 圆 arrow-up 重发 = revert before + resend)。
 *
 * D-038:`msg.source === 'receipt'` 是系统唤醒轮——正文是系统生成的「回执已送达」说明,
 * 不是用户说的话:虚线边 + 「回执唤醒」chip,不提供编辑重发(重发一段系统话没有意义,
 * 而且 revert 会把已送达的回执卡片一起截掉)。
 *
 * D-044:UltraPlan 流程内的消息同样不可编辑重发——卡片动作的回显(msg.ultraAction,正文是
 * 后端写的展示文案),以及流程进行中时以 ultraplan 模式发出的消息。编辑重发 = 回退事件日志,
 * 而阶段机不随日志回退:卡片从时间线上消失、阶段却还停在后面(后端对此回 409
 * ULTRAPLAN_REVERT_BLOCKED)。要重来用状态条上的「重新开始」。
 */
export default function UserMessageCard({ msg }: { msg: ChatMsg }) {
  const activeRunId = useChatStore((st) => st.activeRunId);
  const editAndResend = useChatStore((st) => st.editAndResend);
  const agentKind = useSessionStore((st) => {
    const s = st.sessions.find((x) => x.id === st.activeSessionId);
    return s?.agentKind ?? 'coding';
  });
  const agentEngine = useSessionStore(
    (st) =>
      st.sessions.find((x) => x.id === st.activeSessionId)?.agentEngine ??
      st.draftAgentEngine,
  );
  const [editRequested, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [editMode, setEditMode] = useState('build');
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const cardRef = useRef<HTMLDivElement>(null);

  const modeMeta = COMPOSER_MODES.find((m) => m.id === msg.mode);
  const editModes = modesForKind(agentKind, agentEngine);
  const editModeMeta = composerModeMeta(editMode);
  const isWake = msg.source === 'receipt';
  const steeringMessage = useCollaborationStore((state) => state.messages.find((message) => message.id === msg.messageId));
  const isSteering = !!msg.messageId || !!msg.clientMessageId;
  const { flow } = useFlowContext();
  const ultraLocked =
    msg.ultraAction !== undefined ||
    (msg.mode === 'ultraplan' && flow !== null && flow.stage !== 'done');
  const locked = isWake || ultraLocked || isSteering;
  // 编辑到一半卡片变成不可回退(流程开始了):编辑态随之收起,不留一个没有出口的输入框。
  const editing = editRequested && !locked;
  // 收起就是取消:之后流程结束、卡片解锁时,不把当时那份草稿的编辑框再弹出来。
  useEffect(() => {
    if (locked) {
      setEditing(false);
      setModeMenuOpen(false);
    }
  }, [locked]);
  const startEdit = () => {
    if (locked) return;
    if (activeRunId) {
      useToastStore.getState().push('error', '当前有任务运行中，请先等待完成或中止');
      return;
    }
    setDraft(msg.text);
    // 原消息的 mode 可能已不在当前可用档位内(kind/engine 改过)→ 回落 Agent
    setEditMode(editModes.some((m) => m.id === msg.mode) ? (msg.mode as string) : 'build');
    setModeMenuOpen(false);
    setEditing(true);
  };

  useEffect(() => {
    if (!modeMenuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (cardRef.current && !cardRef.current.contains(e.target as Node)) {
        setModeMenuOpen(false);
      }
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [modeMenuOpen]);

  const resend = () => {
    const text = draft.trim();
    if (text === '') {
      useToastStore.getState().push('error', '请输入新的消息内容');
      return;
    }
    setEditing(false);
    setModeMenuOpen(false);
    void editAndResend(msg.id, text, editMode);
  };

  return (
    <div
      ref={cardRef}
      data-testid="user-message-card"
      data-source={msg.source ?? 'user'}
      data-locked={ultraLocked ? 'ultraplan' : undefined}
      className={cn(
        'group/user relative flex flex-col gap-1.5 rounded-xl border px-3 pb-2 pt-2.5 shadow-sh1',
        isWake
          ? 'border-dashed border-edge bg-shell-sunk'
          : editing
            ? 'border-acc-ring bg-shell-panel'
            : ultraLocked
              ? 'border-edge bg-shell-panel'
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
      {!!msg.annotations?.length && <AnnotationChips annotations={msg.annotations} />}
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
          className={cn(
            'whitespace-pre-wrap text-[13px]',
            isWake ? 'text-fg-2' : ultraLocked ? 'text-fg' : 'cursor-pointer text-fg',
          )}
          data-testid="user-message-text"
          title={isSteering ? '运行中的引导消息' : isWake ? undefined : ultraLocked ? ULTRA_LOCK_TITLE : '点击编辑并重发'}
          onClick={startEdit}
        >
          {msg.text}
        </div>
      )}
      {/* 底条:mode chip + 动作 */}
      <div className="flex items-center gap-1.5 pt-0.5">
        {isSteering && <span data-testid="user-steering-chip" className="text-[11px] text-fg-3">
          引导 · {msg.agentName ?? '主 agent'} · {MESSAGE_STATUS_LABEL[(steeringMessage?.status ?? msg.messageStatus ?? 'queued') as AgentMessageStatus] ?? msg.messageStatus}
        </span>}
        {isWake && (
          <span
            data-testid="user-wake-chip"
            className="flex h-5 items-center gap-1 rounded-full bg-acc-bg px-2 text-[11px] text-acc"
          >
            <BellRing size={11} />
            回执唤醒
          </span>
        )}
        {editing ? (
          <div className="relative">
            <button
              type="button"
              aria-label="更改模式"
              aria-expanded={modeMenuOpen}
              data-testid="user-edit-mode"
              onClick={() => setModeMenuOpen((v) => !v)}
              className="flex h-5 items-center gap-1 rounded-full bg-acc-bg px-2 text-[11px] text-acc"
            >
              <editModeMeta.icon size={11} className={editModeMeta.iconClassName} />
              {editModeMeta.label}
              <ChevronDown size={9} />
            </button>
            {modeMenuOpen && (
              <div
                role="menu"
                data-testid="user-edit-mode-menu"
                className="absolute bottom-full left-0 z-40 mb-1 flex min-w-[132px] flex-col rounded-[10px] border border-edge bg-shell-float p-1 shadow-float"
              >
                {editModes.map((m) => (
                  <button
                    key={m.id}
                    type="button"
                    role="menuitem"
                    data-testid={`user-edit-mode-${m.id}`}
                    onClick={() => {
                      setEditMode(m.id);
                      setModeMenuOpen(false);
                    }}
                    className={cn(
                      'flex h-[24px] items-center gap-2 rounded-md px-2 text-left text-[12px] hover:bg-shell-selection',
                      m.id === editMode ? 'text-acc' : 'text-fg-2',
                    )}
                  >
                    <m.icon size={11} className={m.iconClassName} />
                    <span className="min-w-0 flex-1 truncate">{m.label}</span>
                    {m.id === editMode && <Check size={10} />}
                  </button>
                ))}
              </div>
            )}
          </div>
        ) : (
          <span className="flex h-5 items-center gap-1 rounded-full bg-shell-sunk px-2 text-[11px] text-fg-3">
            {modeMeta && <modeMeta.icon size={11} className={modeMeta.iconClassName} />}
            {modeMeta?.label ?? 'Agent'}
          </span>
        )}
        <span className="flex-1" />
        {!editing && msg.text.trim() !== '' && (
          <button
            type="button"
            aria-label="复制消息"
            title="复制消息"
            data-testid="user-copy"
            onClick={() => void copyText(msg.text, '消息')}
            className="flex h-6 w-6 items-center justify-center rounded-full text-fg-3 opacity-0 transition-opacity hover:bg-shell-hover focus-visible:opacity-100 group-hover/user:opacity-100"
          >
            <Copy size={11} />
          </button>
        )}
        {locked ? null : editing ? (
          <>
            <button
              type="button"
              aria-label="取消编辑"
              data-testid="user-edit-cancel"
              onClick={() => {
                setEditing(false);
                setModeMenuOpen(false);
              }}
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
