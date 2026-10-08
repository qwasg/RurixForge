import { useChatStore, type TodoItem } from '@/lib/chatStore';
import TodoStatusIndicator from '@/components/workbench/TodoStatusIndicator';
import { normalizeTodoStatus, type TodoStatus } from '@/lib/todoStatus';

/**
 * F7 wave.5 Todo tab(参考 render_todo_page 四列看板):
 * Backlog=queued / Running=running / Review=failed / Done=completed;
 * 卡 = bg_panel + line + sh1:状态点 + 标题 + description(11px 两行截断);
 * 只读(本仓 todo 无 run 语义,不造重跑钮;参考的 rerun 依赖 active_run + action 面,不落)。
 */

const COLUMNS: Array<{ id: string; label: string; status: TodoStatus }> = [
  { id: 'backlog', label: 'Backlog', status: 'pending' },
  { id: 'running', label: 'Running', status: 'running' },
  { id: 'review', label: 'Review', status: 'failed' },
  { id: 'done', label: 'Done', status: 'completed' },
];

/** 待办来源标记(todo.source);Agent 在对话中建的待办无来源字段,不画标记。 */
const SOURCE_LABEL: Record<string, string> = { plan: '计划', user: '用户', ultraplan: 'UltraPlan' };
/** 来源标记的悬停说明(D-044:UltraPlan 制作阶段物化的任务另有一句)。 */
const SOURCE_TITLE: Record<string, string> = {
  plan: '来自计划文件',
  user: '手动创建',
  ultraplan: '来自 UltraPlan 计划',
};

function KanbanColumn({ label, items }: { label: string; items: TodoItem[] }) {
  return (
    <div className="flex min-w-[180px] flex-1 flex-col gap-1.5 rounded-[10px] bg-shell-sunk p-2" data-testid={`todo-col-${label}`}>
      <div className="flex items-center gap-1.5 pb-0.5 text-[11px] font-semibold text-fg-3">
        {label}
        <span className="rounded-full bg-shell-active px-[5px] font-code text-[10px]">{items.length}</span>
      </div>
      {items.map((t) => (
        <div
          key={t.id}
          data-testid={`todo-card-${t.id}`}
          className="flex flex-col gap-1 rounded-lg border border-edge bg-shell-panel p-2 shadow-sh1"
        >
          <div className="flex items-center gap-1.5">
            <TodoStatusIndicator status={t.status} />
            <span className="min-w-0 flex-1 truncate text-[12.5px] text-fg">
              {t.title === '' ? t.id : t.title}
            </span>
          </div>
          {t.description?.trim() ? (
            <div
              className="overflow-hidden text-[11px] text-fg-3"
              style={{ display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' }}
            >
              {t.description}
            </div>
          ) : null}
          <div className="flex items-center gap-1">
            {SOURCE_LABEL[t.source ?? ''] && (
              <span
                data-testid={`todo-source-${t.id}`}
                title={SOURCE_TITLE[t.source ?? '']}
                className="rounded-full bg-acc-bg px-1.5 text-[9.5px] leading-4 text-acc"
              >
                {SOURCE_LABEL[t.source ?? '']}
              </span>
            )}
            <span className="text-[10px] text-fg-4">{t.status}</span>
          </div>
        </div>
      ))}
    </div>
  );
}

export default function TodoTab() {
  const todos = useChatStore((st) => st.todos);
  return (
    <div data-testid="todo-tab" className="flex h-full min-h-0 flex-col gap-2.5 overflow-y-auto p-5">
      <span className="font-serif text-[24px] font-bold text-fg">Todo</span>
      {todos.length === 0 && <span className="text-[12px] text-fg-4">暂无待办。</span>}
      <div className="flex items-start gap-2.5">
        {COLUMNS.map((c) => (
          <KanbanColumn
            key={c.id}
            label={c.label}
            items={todos.filter((t) => normalizeTodoStatus(t.status) === c.status)}
          />
        ))}
      </div>
    </div>
  );
}
