import { useEffect, useRef, useState } from 'react';
import { FileCode2, GitBranch, MoreHorizontal, PanelsTopLeft, Sparkles } from 'lucide-react';
import { useChatStore } from '@/lib/chatStore';
import { HOME_COL_MAX, type ChatVariant } from '@/lib/chatVariant';
import { cn } from '@/lib/cn';
import { runCommand } from '@/lib/commands';
import { useComposerPrefillStore } from '@/lib/composerStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { ChatMiniBtn, IBtn, MenuItem, PaneToggleBtn } from './primitives';
import { StatusDot } from './primitives';
import MessageList from '../chat/MessageList';
import Composer from '../chat/Composer';
import SubagentOverlay from '../chat/SubagentOverlay';

/**
 * F7 wave.4 对话列(参考 ui/chat.rs render_chat_column):
 * 38px 头(状态点/标题/fork/more[重命名/删除 真实接线])+
 * MessageList(SSE 驱动)+ Composer 全量 + SubagentOverlay(浮于 composer 上方)。
 * 会话切换 → chatStore.selectSession(快照回放 + SSE 订阅;切换关旧流)。
 *
 * home 变体(2026-08-24 用户拍板):workbench 没有 tab 时同一套组件接管整屏——
 * 头右侧换成「工作台」出口,正文/输入收在 HOME_COL_MAX 居中列,
 * 零消息时输入上方立起 Codex 式首屏(问候 + 大输入盒 + 快捷起手式)。
 * 两个变体的子树形状保持一致,切换不重挂 Composer(草稿/模式不丢)。
 *
 * mini 变体(2026-08-25 用户拍板):对话收成主区左下角浮窗,正文形态与 column 同,
 * 只换浮窗底色;头上的缩小钮换成还原钮(见 ChatMiniBtn)。
 * 窗头同时是拖把手(data-mini-drag):按住拖走浮窗、双击归位,搬运逻辑在 Shell。
 */
export default function ChatColumn({ variant = 'column' }: { variant?: ChatVariant }) {
  const home = variant === 'home';
  const mini = variant === 'mini';
  const activeSessionId = useSessionStore((st) => st.activeSessionId);
  const activeSession = useSessionStore((st) => st.sessions.find((s) => s.id === st.activeSessionId));
  const fork = useSessionStore((st) => st.fork);
  const remove = useSessionStore((st) => st.remove);
  const rename = useSessionStore((st) => st.rename);
  const activeRunId = useChatStore((st) => st.activeRunId);
  const msgCount = useChatStore((st) => st.messages.length);
  const hydrating = useChatStore((st) => st.hydrating);
  const setHomeDismissed = useWorkbenchStore((st) => st.setHomeDismissed);

  const [menuOpen, setMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState('');
  const headRef = useRef<HTMLDivElement>(null);

  /** 首屏:全屏主页 + 当前会话没有任何消息(拉快照期间不闪)。 */
  const hero = home && msgCount === 0 && !hydrating;

  // 会话切换 → 快照回放 + SSE 订阅(chatStore 内关旧流);
  // 已对齐的会话跳过——Composer「无会话直发」自己订过一次,重订会 reset 掉乐观回显。
  useEffect(() => {
    if (useChatStore.getState().currentSessionId === activeSessionId) return;
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
    <div
      data-testid="chat-column"
      data-variant={variant}
      className={cn(
        'relative flex h-full min-h-0 flex-col',
        home ? 'bg-shell-bg' : mini ? 'bg-shell-float' : 'bg-shell-panel',
      )}
    >
      {/* 38px 头(mini 时兼作浮窗拖把手:标记由 Shell 的事件委派认) */}
      <div
        ref={headRef}
        data-mini-drag={mini ? '' : undefined}
        title={mini ? '按住拖动窗口,双击回到左下角' : undefined}
        className={cn(
          'relative flex h-[38px] shrink-0 items-center gap-1.5 border-b border-edge',
          home ? 'px-4' : 'px-3',
          mini && 'cursor-grab select-none active:cursor-grabbing',
        )}
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
        {home ? (
          <button
            type="button"
            title="打开工作台(编辑器/文件/面板)"
            data-testid="home-to-workbench"
            onClick={() => setHomeDismissed(true)}
            className="flex h-[26px] shrink-0 items-center gap-1 rounded-md border border-edge px-2 text-[11px] text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2"
          >
            <PanelsTopLeft size={12} />
            工作台
          </button>
        ) : (
          <>
            <ChatMiniBtn className="h-[26px] w-[26px]" />
            <PaneToggleBtn kind="chat" className="h-[26px] w-[26px]" />
          </>
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

      {/* 消息流(首屏时让位给 hero)+ Composer + 子代理浮层 */}
      {hero ? <HomeHero /> : <MessageList variant={variant} />}
      <div className={cn('shrink-0', home && 'px-6 pb-6')}>
        <div
          className={cn(home && 'mx-auto w-full')}
          style={home ? { maxWidth: HOME_COL_MAX } : undefined}
        >
          <Composer variant={variant} />
          {hero && <QuickStarts />}
        </div>
      </div>
      {/* 首屏配平:上方 hero 撑 1、下方留白撑 0.8,输入盒落在略高于视觉中线处 */}
      {hero && <div className="min-h-0 flex-[0.8]" />}
      <SubagentOverlay variant={variant} />
    </div>
  );
}

/** 全屏主页首屏问候(贴在大输入盒正上方,整体略高于视觉中线)。 */
function HomeHero() {
  return (
    <div
      data-testid="home-hero"
      className="flex min-h-0 flex-1 flex-col items-center justify-end overflow-y-auto px-6 pb-7"
    >
      <div className="w-full" style={{ maxWidth: HOME_COL_MAX }}>
        <p className="font-serif text-[30px] font-semibold leading-tight text-fg">
          今天想搭点什么？
        </p>
        <p className="mt-2 text-[13px] text-fg-3">
          直接描述目标，Agent 会拆成计划再动手；需要看场景时随时打开编辑器。
        </p>
      </div>
    </div>
  );
}

/** 起手式:点一下把话术灌进输入框(不直接发,留给人改)。 */
const QUICK_STARTS: Array<{ id: string; label: string; draft: string; mode: string }> = [
  {
    id: 'greybox',
    label: '搭一个灰盒关卡',
    draft: '搭一个灰盒关卡：地面、三级平台和一个出生点',
    mode: 'build',
  },
  {
    id: 'plan',
    label: '先出一份计划',
    draft: '给我一份从零做出可玩 demo 的分步计划',
    mode: 'plan',
  },
  {
    id: 'debug',
    label: '排查视口没画面',
    draft: '视口没有渲染出实体，帮我定位原因',
    mode: 'debug',
  },
];

function QuickStarts() {
  const prefill = useComposerPrefillStore((st) => st.prefill);
  const chip =
    'flex h-[24px] items-center gap-1 rounded-full border border-edge bg-shell-panel px-2.5 text-[11px] text-fg-3 transition-colors hover:border-edge-strong hover:text-fg-2';
  return (
    <div data-testid="home-quick-starts" className="mt-2.5 flex flex-wrap items-center gap-1.5 px-1">
      {QUICK_STARTS.map((q) => (
        <button
          key={q.id}
          type="button"
          data-testid={`home-quick-${q.id}`}
          onClick={() => prefill(q.draft, q.mode)}
          className={chip}
        >
          <Sparkles size={11} />
          {q.label}
        </button>
      ))}
      <span className="flex-1" />
      <button
        type="button"
        data-testid="home-open-editor"
        onClick={() => runCommand('tab.editor')}
        className={chip}
      >
        <FileCode2 size={11} />
        打开编辑器
      </button>
    </div>
  );
}
