import { Check } from 'lucide-react';
import MarkdownFlat from '@/components/chat/MarkdownFlat';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import { cn } from '@/lib/cn';
import { useComposerPrefillStore } from '@/lib/composerStore';
import { StatusDot } from '@/components/shell/primitives';

/**
 * F7 wave.5 Plan tab(参考 render_plan_page 适配本仓语义):
 * 本仓无 Plan 实体(plan 是只读聊天模式)→ 展示最近一次 plan 模式 turn 的 assistant
 * 终稿 Markdown(chatStore 消息里 mode==='plan' 的用户消息所对应 run 的 assistant 终稿);
 * 下方 To-dos 表(chatStore.todos:状态圆点+标题,完成划线);无内容空态「尚无计划」。
 * 「开始 Build」:切 composer 到 build 模式 + 预填「执行上述计划」并聚焦(真实行为,非占位)。
 *
 * 差异留痕:参考 Plan 页有 breadcrumb/模型菜单/find 栏/DAG/timeline/diff 历史——本仓
 * 无 plan 实体数据面,全部不落;仅终稿 md + To-dos 表 + 开始 Build 三件套。
 */

/** 最近一次 plan 模式 turn 的 assistant 终稿文本(无 → null)。 */
export function lastPlanText(messages: ChatMsg[]): string | null {
  // runId → 用户消息 mode 映射
  const modeByRun = new Map<string, string>();
  for (const m of messages) {
    if (m.role === 'user' && m.runId && m.mode) modeByRun.set(m.runId, m.mode);
  }
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const m = messages[i];
    if (m.role !== 'assistant') continue;
    const mode = m.runId ? modeByRun.get(m.runId) : undefined;
    if (mode !== 'plan') continue;
    // 终稿 = 末尾 final text 块(或全部 text 块拼合兜底)
    const texts = m.blocks.filter((b) => b.kind === 'text').map((b) => (b as { text: string }).text);
    const text = texts.join('\n\n').trim();
    if (text !== '') return text;
  }
  return null;
}

function TodoTable() {
  const todos = useChatStore((st) => st.todos);
  return (
    <div className="flex flex-col gap-1.5 border-t border-edge pt-3" data-testid="plan-todo-table">
      <span className="text-[11px] font-semibold text-fg-3">{todos.length} To-dos</span>
      {todos.length === 0 && <span className="py-1 text-[12px] text-fg-4">暂无待办</span>}
      {todos.map((t) => {
        const done = t.status === 'completed' || t.status === 'done';
        const running = t.status === 'running' || t.status === 'in_progress';
        return (
          <div key={t.id} className="flex items-center gap-2 py-1" data-testid={`plan-todo-${t.id}`}>
            {done ? (
              <span className="flex h-4 w-4 items-center justify-center rounded-full bg-acc-bg">
                <Check size={10} className="text-acc" />
              </span>
            ) : running ? (
              <StatusDot color="var(--dot-running)" pulse />
            ) : (
              <span className="h-4 w-4 rounded-full border border-edge" />
            )}
            <span
              className={cn(
                'min-w-0 flex-1 truncate text-[13px]',
                done ? 'text-fg-3 line-through' : 'text-fg',
              )}
            >
              {t.title === '' ? t.id : t.title}
            </span>
          </div>
        );
      })}
    </div>
  );
}

export default function PlanTab() {
  const messages = useChatStore((st) => st.messages);
  const activeRunId = useChatStore((st) => st.activeRunId);
  const prefill = useComposerPrefillStore((st) => st.prefill);
  const planText = lastPlanText(messages);
  const canBuild = planText !== null && activeRunId === null;

  return (
    <div data-testid="plan-tab" className="flex h-full min-h-0 flex-col">
      {/* 头:标题 + 开始 Build */}
      <div className="flex shrink-0 items-start gap-3 border-b border-edge p-5 pb-3">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <span className="font-serif text-[24px] font-bold text-fg">Plan</span>
          <span className="truncate text-[11px] text-fg-3">最近一次 plan 模式会话的终稿</span>
        </div>
        <button
          type="button"
          data-testid="plan-start-build"
          disabled={!canBuild}
          onClick={() => prefill('执行上述计划', 'build')}
          className={cn(
            'flex h-[28px] items-center rounded-md border px-2.5 text-[12px]',
            canBuild
              ? 'border-acc bg-acc text-fg-inv hover:bg-acc-soft'
              : 'cursor-not-allowed border-edge bg-shell-panel text-fg-4',
          )}
        >
          开始 Build
        </button>
      </div>
      {/* 正文 */}
      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4" data-testid="plan-body">
        {planText === null ? (
          <div className="flex flex-col items-center gap-2 pt-12">
            <span className="font-serif text-[24px] text-fg">尚无计划</span>
            <span className="text-[12px] text-fg-3">在对话中以 Plan 模式发送任务后，计划将在此展示。</span>
          </div>
        ) : (
          <MarkdownFlat text={planText} />
        )}
      </div>
      {/* To-dos 表 */}
      <div className="shrink-0 px-5 pb-5">
        <TodoTable />
      </div>
    </div>
  );
}
