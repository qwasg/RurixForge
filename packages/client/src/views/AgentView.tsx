import { useEffect, useRef, useState } from 'react';
import {
  ArrowDown,
  Check,
  ChevronDown,
  Copy,
  GitBranch,
  GitFork,
  Monitor,
  Moon,
  Sparkles,
  ThumbsDown,
  ThumbsUp,
} from 'lucide-react';
import { useAppStore } from '@/lib/store';
import { AGENT_BRANCH, CONVERSATION } from '@/lib/mock';
import type { ConversationBlock } from '@/lib/types';
import { cn } from '@/lib/cn';
import Composer from '@/components/Composer';
import RightPanel from '@/components/RightPanel';
import Markdown from '@/components/agent/Markdown';
import SubagentList, { type SubagentPhase } from '@/components/agent/SubagentList';

/* ---------------- 模拟节奏(视频 2) ---------------- */

const SUBAGENT_IDS = CONVERSATION.flatMap((b) =>
  b.kind === 'subagents' ? b.items.map((i) => i.id) : [],
);
/** 最后一个 markdown 块与其前的 thought:全部 done 后再出现 */
const FINAL_MD_INDEX = CONVERSATION.length - 1;
const FINAL_THOUGHT_INDEX = CONVERSATION.length - 2;
const RUNNING_AT = 1500; // 挂载 1.5s 后 3 个 running
const DONE_BASE = 8000; // 8s / 10s / 12s 依次 done
const DONE_STAGGER = 2000;
const FINAL_REVEAL_DELAY = 1200;

/* ---------------- 小组件 ---------------- */

const iconBtn =
  'grid h-7 w-7 place-items-center rounded-md text-muted transition-colors hover:bg-panel-hover hover:text-ink-soft';

/** 'Thought briefly' 行:悬停出现 chevron,点击展开。 */
function ThoughtRow({ label }: { label: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="group flex items-center gap-1 text-sm italic text-muted"
      >
        {label}
        <ChevronDown
          size={12}
          className={cn(
            'text-muted-faint opacity-0 transition group-hover:opacity-100',
            open && 'rotate-180 opacity-100',
          )}
        />
      </button>
      {open && <div className="mt-0.5 text-xs italic text-muted-faint">Thought for a few seconds</div>}
    </div>
  );
}

/** 最终回复下方的反馈行(Just now + 赞/踩/分叉/复制)。 */
function FeedbackRow({ md }: { md: string }) {
  const [vote, setVote] = useState<'up' | 'down' | null>(null);
  const [copied, setCopied] = useState(false);
  const onCopy = () => {
    try {
      void navigator.clipboard?.writeText(md);
    } catch {
      /* 剪贴板不可用时静默 */
    }
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  const actBtn = 'grid h-6 w-6 place-items-center rounded-md text-muted-faint transition-colors hover:bg-panel-hover hover:text-muted';
  return (
    <div className="mt-2 flex items-center text-xs text-muted-faint">
      <span>Just now</span>
      <div className="ml-auto flex items-center gap-0.5">
        <button
          type="button"
          className={cn(actBtn, vote === 'up' && 'text-ink')}
          onClick={() => setVote((v) => (v === 'up' ? null : 'up'))}
          title="Good response"
        >
          <ThumbsUp size={13} />
        </button>
        <button
          type="button"
          className={cn(actBtn, vote === 'down' && 'text-ink')}
          onClick={() => setVote((v) => (v === 'down' ? null : 'down'))}
          title="Bad response"
        >
          <ThumbsDown size={13} />
        </button>
        <button type="button" className={actBtn} title="Fork conversation">
          <GitFork size={13} />
        </button>
        <button type="button" className={actBtn} onClick={onCopy} title="Copy">
          {copied ? <Check size={13} className="text-accent-green" /> : <Copy size={13} />}
        </button>
      </div>
    </div>
  );
}

/* ---------------- 主视图 ---------------- */

export default function AgentView() {
  const rightPanelOpen = useAppStore((s) => s.rightPanelOpen);

  // 子智能体阶段模拟:starting →(1.5s)running →(8/10/12s)done
  const [phases, setPhases] = useState<Record<string, SubagentPhase>>(() =>
    Object.fromEntries(SUBAGENT_IDS.map((id) => [id, 'starting' as SubagentPhase])),
  );
  const [revealFinal, setRevealFinal] = useState(false);
  const [sent, setSent] = useState<ConversationBlock[]>([]);

  useEffect(() => {
    const timers = [
      window.setTimeout(() => {
        setPhases((p) => {
          const next = { ...p };
          for (const id of SUBAGENT_IDS) if (next[id] === 'starting') next[id] = 'running';
          return next;
        });
      }, RUNNING_AT),
      ...SUBAGENT_IDS.map((id, i) =>
        window.setTimeout(
          () => setPhases((p) => (p[id] === 'running' ? { ...p, [id]: 'done' } : p)),
          DONE_BASE + i * DONE_STAGGER,
        ),
      ),
    ];
    return () => timers.forEach((t) => window.clearTimeout(t));
  }, []);

  const runningCount = SUBAGENT_IDS.filter((id) => phases[id] === 'running').length;
  const allDone = SUBAGENT_IDS.length > 0 && SUBAGENT_IDS.every((id) => phases[id] === 'done');

  useEffect(() => {
    if (!allDone) return;
    const t = window.setTimeout(() => setRevealFinal(true), FINAL_REVEAL_DELAY);
    return () => window.clearTimeout(t);
  }, [allDone]);

  // 消息区自动吸附底部;用户上翻后出现 ↓ 跳转钮
  const scrollRef = useRef<HTMLDivElement>(null);
  const stickRef = useRef(true);
  const [showJump, setShowJump] = useState(false);
  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const dist = el.scrollHeight - el.scrollTop - el.clientHeight;
    stickRef.current = dist < 80;
    setShowJump(dist > 160);
  };
  const scrollToBottom = () => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  };
  useEffect(() => {
    if (stickRef.current) scrollToBottom();
  }, [phases, revealFinal, sent]);

  const onStopSubagent = (id: string) =>
    setPhases((p) => (p[id] === 'running' ? { ...p, [id]: 'done' } : p));
  const onSend = (text: string) => {
    if (!text.trim()) return;
    stickRef.current = true;
    setSent((s) => [...s, { kind: 'user', text }]);
  };

  const renderBlock = (block: ConversationBlock, key: string) => {
    switch (block.kind) {
      case 'user':
        return (
          <div key={key}>
            <div className="inline-block max-w-full whitespace-pre-wrap rounded-2xl bg-panel-hover px-3 py-2 text-base text-ink">
              {block.text}
            </div>
          </div>
        );
      case 'thought':
        return <ThoughtRow key={key} label={block.label} />;
      case 'muted-line':
      case 'waiting':
        return (
          <div key={key} className="text-sm italic text-muted">
            {block.label}
          </div>
        );
      case 'markdown':
        return (
          <div key={key} className="py-1">
            <Markdown md={block.md} />
          </div>
        );
      case 'subagents':
        return (
          <SubagentList key={key} items={block.items} phases={phases} onStop={onStopSubagent} />
        );
    }
  };

  const finalMd =
    CONVERSATION[FINAL_MD_INDEX]?.kind === 'markdown' ? CONVERSATION[FINAL_MD_INDEX].md : '';

  return (
    <div className="flex h-full min-h-0 flex-col bg-white">
      <div className="flex min-h-0 flex-1">
        {/* 会话列 */}
        <div className="flex min-w-0 flex-1 flex-col">
          <div ref={scrollRef} onScroll={onScroll} className="min-h-0 flex-1 overflow-y-auto">
            <div className="mx-auto flex max-w-[760px] flex-col gap-2.5 px-6 pb-8 pt-5">
              {CONVERSATION.map((block, i) => {
                if (block.kind === 'waiting' && allDone) return null;
                if (i === FINAL_THOUGHT_INDEX && block.kind === 'thought' && !allDone) return null;
                if (i === FINAL_MD_INDEX && block.kind === 'markdown' && !revealFinal) return null;
                return renderBlock(block, `c${i}`);
              })}
              {revealFinal && finalMd && <FeedbackRow md={finalMd} />}
              {sent.map((block, i) => renderBlock(block, `s${i}`))}
            </div>
          </div>

          {/* 底部:Working chip / Commit & Push + Composer + 分支行 */}
          <div className="shrink-0 pb-3">
            <div className="mx-auto max-w-[760px] px-6">
              {(runningCount > 0 || allDone || showJump) && (
                <div className="mb-2 flex h-7 items-center gap-2">
                  {runningCount > 0 && (
                    <>
                      <span className="flex items-center gap-1.5 rounded-full border border-line bg-white px-2.5 py-1 text-xs text-ink-soft">
                        <span className="relative">
                          <Sparkles size={12} className="text-ink-soft" />
                          <span className="absolute -right-0.5 -top-0.5 h-1.5 w-1.5 rounded-full bg-accent-green" />
                        </span>
                        {runningCount} Working
                      </span>
                      <button
                        type="button"
                        onClick={scrollToBottom}
                        className="grid h-5 w-5 place-items-center rounded-full border border-line text-muted transition-colors hover:bg-panel-hover"
                        title="Jump to latest"
                      >
                        <ChevronDown size={11} />
                      </button>
                    </>
                  )}
                  {allDone && (
                    <button type="button" className="btn-primary">
                      Commit &amp; Push
                      <ChevronDown size={12} />
                    </button>
                  )}
                  {showJump && (
                    <button
                      type="button"
                      onClick={scrollToBottom}
                      className="grid h-6 w-6 place-items-center rounded-full border border-line bg-white text-muted shadow-sm transition-colors hover:bg-panel-hover"
                      title="Jump to latest"
                    >
                      <ArrowDown size={12} />
                    </button>
                  )}
                </div>
              )}

              <Composer placeholder="Send follow-up" onSend={onSend} />

              <div className="mt-2 flex items-center justify-between">
                <button
                  type="button"
                  className="flex items-center gap-1 rounded-full px-2 py-1 text-xs text-muted transition-colors hover:bg-panel-hover hover:text-ink-soft"
                >
                  <GitBranch size={12} />
                  <span className="truncate">{AGENT_BRANCH}</span>
                  <ChevronDown size={12} className="text-muted-faint" />
                </button>
                <div className="flex items-center gap-1">
                  <button
                    type="button"
                    className="flex items-center gap-1 rounded-full px-2 py-1 text-xs text-muted transition-colors hover:bg-panel-hover hover:text-ink-soft"
                  >
                    <Monitor size={12} />
                    <span>This PC</span>
                    <ChevronDown size={12} className="text-muted-faint" />
                  </button>
                  <button type="button" className={cn(iconBtn, 'h-6 w-6')} title="Theme">
                    <Moon size={13} />
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>

        {rightPanelOpen && <RightPanel />}
      </div>
    </div>
  );
}
