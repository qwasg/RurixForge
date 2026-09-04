import { useEffect, useMemo, useState } from 'react';
import { Check, Eye, Hammer, Pencil, Search } from 'lucide-react';
import MarkdownFlat from '@/components/chat/MarkdownFlat';
import ModelPicker from '@/components/chat/ModelPicker';
import CodeEditor from '@/components/workbench/CodeEditor';
import { StatusDot } from '@/components/shell/primitives';
import { useChatStore } from '@/lib/chatStore';
import { cn } from '@/lib/cn';
import { parsePlanFile, planNameFromPath, type PlanTodo } from '@/lib/planFile';
import { buildPrompt, usePlanStore } from '@/lib/planStore';
import { useFileEditor } from '@/lib/useFileEditor';
import { useToastStore } from '@/lib/toastStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * D-035 Plan 页签:计划文件(.forge/plans/<名>.plan.md)的阅读 / 编辑 / 执行面。
 *
 * 事实源是文件本身,不是聊天终稿——加载/保存/dirty/草稿/EOL/409 冲突全套复用
 * useFileEditor(与工作区文件编辑器同一份实现)。
 * 头部右端 = 预览/编辑切换 + 模型切换器 + Build 主按钮:换了模型再点 Build,
 * 后端就以新模型开这一轮实施(模型是会话级的,PATCH 即生效)。
 * 待办清单单独渲染 front matter 的 todos,不走 Markdown task-list——MarkdownFlat
 * 不支持勾选框/嵌套列表(已知限制),清单是结构化数据,本就不该塞回文本里。
 */

/** 会话待办按 planTodoId 建索引:Build 之后计划清单显示真实执行状态。 */
function useTodoStatusByPlanId(): Map<string, string> {
  const todos = useChatStore((st) => st.todos);
  return useMemo(() => {
    const m = new Map<string, string>();
    for (const t of todos) {
      if (t.planTodoId) m.set(t.planTodoId, t.status);
    }
    return m;
  }, [todos]);
}

function TodoRow({ todo, status }: { todo: PlanTodo; status: string }) {
  const done = status === 'completed' || status === 'done';
  const running = status === 'running' || status === 'in_progress';
  const failed = status === 'failed' || status === 'blocked';
  return (
    <div className="flex items-start gap-2 py-1" data-testid={`plan-todo-${todo.id}`}>
      {done ? (
        <span className="mt-px flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-acc-bg">
          <Check size={10} className="text-acc" />
        </span>
      ) : running ? (
        <span className="mt-1.5 flex h-4 w-4 shrink-0 items-center justify-center">
          <StatusDot color="var(--dot-running)" pulse />
        </span>
      ) : failed ? (
        <span className="mt-1.5 flex h-4 w-4 shrink-0 items-center justify-center">
          <StatusDot color="var(--dot-blocked)" />
        </span>
      ) : (
        <span className="mt-px h-4 w-4 shrink-0 rounded-full border border-edge" />
      )}
      <span
        className={cn(
          'min-w-0 flex-1 text-[13px]',
          done ? 'text-fg-3 line-through' : failed ? 'text-warn' : 'text-fg',
        )}
      >
        {todo.content}
      </span>
    </div>
  );
}

export default function PlanTab({ path, tabId }: { path: string; tabId: string }) {
  const reloadNonce = usePlanStore((st) => st.reloadNonce);
  const planning = usePlanStore((st) => st.planning);
  const activeRunId = useChatStore((st) => st.activeRunId);
  const setTabTitle = useWorkbenchStore((st) => st.setTabTitle);
  const forceCloseTab = useWorkbenchStore((st) => st.forceCloseTab);
  const cancelCloseTab = useWorkbenchStore((st) => st.cancelCloseTab);
  const closeConfirm = useWorkbenchStore((st) => st.pendingCloseTabId === tabId);
  const statusByPlanId = useTodoStatusByPlanId();
  const [editing, setEditing] = useState(false);
  const [building, setBuilding] = useState(false);
  // 编辑期间的实时草稿:切回预览要看到刚改的内容,不必先保存。
  const [draft, setDraft] = useState<string | null>(null);

  const ed = useFileEditor(path, tabId, reloadNonce);

  const source = draft ?? ed.initialDoc;
  const plan = useMemo(() => parsePlanFile(source, path), [source, path]);

  // tabbar 标题跟 front matter 真名走(路径名只是加载前的回落)。
  useEffect(() => {
    if (plan.error === undefined) setTabTitle(tabId, plan.name);
  }, [plan.name, plan.error, tabId, setTabTitle]);

  // 外部重载(agent 覆盖了计划)后草稿失效,回到文件现状。
  useEffect(() => {
    setDraft(null);
  }, [reloadNonce, ed.loadNonce]);

  const progress = useMemo(() => {
    let done = 0;
    for (const t of plan.todos) {
      const s = statusByPlanId.get(t.id) ?? t.status;
      if (s === 'completed' || s === 'done') done += 1;
    }
    return { done, total: plan.todos.length };
  }, [plan.todos, statusByPlanId]);

  const canBuild = !ed.loading && ed.error === null && activeRunId === null && !building;

  const onBuild = async () => {
    if (!canBuild) return;
    setBuilding(true);
    try {
      // dirty 先落盘:后端读的是磁盘上的计划,不能拿旧版本去实施。
      if (ed.dirty && !(await ed.save())) {
        useToastStore.getState().push('error', '计划未能保存,已取消 Build');
        return;
      }
      // 正文只是时间线上的一句话;任务书由后端按 planPath 读计划文件注入(D-034)。
      await useChatStore
        .getState()
        .sendMessage(buildPrompt(plan.name), 'build', undefined, { planPath: path });
    } finally {
      setBuilding(false);
    }
  };

  return (
    <div data-testid="plan-tab" className="flex h-full min-h-0 flex-col bg-shell-bg">
      {/* 头:名称/概述 + 预览-编辑 + 模型 + Build */}
      <div className="flex shrink-0 items-start gap-3 border-b border-edge px-5 py-3">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="truncate font-serif text-[20px] font-bold text-fg" title={plan.name}>
            {ed.loading ? planNameFromPath(path) : plan.name}
          </span>
          <span className="truncate text-[11px] text-fg-3" title={path}>
            {plan.overview !== '' ? plan.overview : path}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {plan.todos.length > 0 && (
            <span data-testid="plan-progress" className="mr-1 font-code text-[11px] text-fg-3">
              {progress.done}/{progress.total}
            </span>
          )}
          <button
            type="button"
            data-testid="plan-toggle-edit"
            onClick={() => setEditing((v) => !v)}
            title={editing ? '切到预览' : '编辑计划'}
            className={cn(
              'flex h-[26px] items-center gap-1 rounded-md border px-2 text-[11.5px] transition-colors',
              editing
                ? 'border-acc-ring bg-acc-bg text-acc'
                : 'border-edge text-fg-2 hover:bg-shell-hover',
            )}
          >
            {editing ? <Eye size={12} /> : <Pencil size={12} />}
            {editing ? '预览' : '编辑'}
          </button>
          <ModelPicker placement="down" align="right" />
          <button
            type="button"
            data-testid="plan-start-build"
            disabled={!canBuild}
            onClick={() => void onBuild()}
            title={activeRunId !== null ? '有任务正在运行' : '按此计划开始实施'}
            className={cn(
              'flex h-[26px] items-center gap-1 rounded-md border px-2.5 text-[12px] font-medium',
              canBuild
                ? 'border-acc bg-acc text-fg-inv hover:bg-acc-soft'
                : 'cursor-not-allowed border-edge bg-shell-panel text-fg-4',
            )}
          >
            <Hammer size={12} />
            Build
          </button>
        </div>
      </div>

      {/* 状态条:调研中 / 未保存 / 解析异常 / 保存失败 */}
      {planning && (
        <div
          data-testid="plan-researching"
          className="flex shrink-0 items-center gap-2 border-b border-edge bg-shell-sunk px-5 py-1 text-[11px] text-fg-2"
        >
          <Search size={11} className="animate-pulse text-acc" />
          正在调研并撰写计划…完成后本页会自动更新。
        </div>
      )}
      {plan.error !== undefined && !ed.loading && ed.error === null && (
        <div
          data-testid="plan-parse-error"
          className="shrink-0 border-b border-edge bg-warn-bg px-5 py-1 text-[11px] text-warn"
        >
          计划头信息解析失败({plan.error}),下面按原文展示;Build 仍会按文件内容执行。
        </div>
      )}
      {closeConfirm && (
        <div
          data-testid="plan-close-confirm"
          className="flex shrink-0 items-center gap-2 border-b border-edge bg-shell-sunk px-5 py-1 text-[11px]"
        >
          <span className="min-w-0 flex-1 truncate text-fg-2">计划有未保存的改动,关闭前要保存吗?</span>
          <button
            type="button"
            data-testid="plan-close-save"
            onClick={() => {
              void ed.save().then((ok) => {
                if (ok) forceCloseTab(tabId);
              });
            }}
            className="shrink-0 rounded border border-edge-strong bg-shell-panel px-1.5 py-px text-fg hover:bg-shell-hover"
          >
            保存并关闭
          </button>
          <button
            type="button"
            data-testid="plan-close-discard"
            onClick={() => forceCloseTab(tabId)}
            className="shrink-0 rounded border border-edge-strong px-1.5 py-px text-warn hover:bg-shell-hover"
          >
            放弃并关闭
          </button>
          <button
            type="button"
            data-testid="plan-close-cancel"
            onClick={cancelCloseTab}
            className="shrink-0 rounded px-1.5 py-px text-fg-3 hover:bg-shell-hover hover:text-fg"
          >
            取消
          </button>
        </div>
      )}
      {ed.saveError !== null && (
        <div
          data-testid="plan-save-error"
          className="flex shrink-0 items-center gap-2 border-b border-edge bg-warn-bg px-5 py-1 text-[11px] text-warn"
        >
          <span className="min-w-0 flex-1 truncate">{ed.saveError}</span>
          <button
            type="button"
            data-testid="plan-reload"
            onClick={ed.reload}
            className="shrink-0 rounded border border-edge-strong px-1.5 py-px hover:bg-shell-hover"
          >
            重新加载(放弃本地改动)
          </button>
        </div>
      )}

      {/* 正文 */}
      <div className="min-h-0 flex-1" data-testid="plan-body">
        {ed.loading && <div className="px-5 py-3 text-[12px] text-fg-4">加载计划…</div>}
        {!ed.loading && ed.error !== null && (
          <div className="flex flex-col items-center gap-2 pt-12" data-testid="plan-load-error">
            <span className="font-serif text-[22px] text-fg">计划打不开</span>
            <span className="px-8 text-center text-[12px] text-warn">{ed.error}</span>
          </div>
        )}
        {!ed.loading && ed.error === null && editing && (
          <CodeEditor
            key={`${tabId}:${ed.loadNonce}`}
            path={path}
            initialDoc={ed.initialDoc}
            onDocChanged={(doc) => {
              setDraft(doc);
              ed.onDocChanged(doc);
            }}
            onSave={() => void ed.save()}
            data-testid="plan-editor"
            className="h-full min-h-0 font-code text-[13px]"
          />
        )}
        {!ed.loading && ed.error === null && !editing && (
          <div className="h-full overflow-y-auto px-5 py-4">
            <MarkdownFlat text={plan.body} />
            <div className="mt-5 flex flex-col gap-1 border-t border-edge pt-3" data-testid="plan-todo-list">
              <span className="pb-0.5 text-[11px] font-semibold text-fg-3">
                {plan.todos.length} 项待办
              </span>
              {plan.todos.length === 0 && (
                <span className="py-1 text-[12px] text-fg-4">该计划没有列出待办。</span>
              )}
              {plan.todos.map((t) => (
                <TodoRow key={t.id} todo={t} status={statusByPlanId.get(t.id) ?? t.status} />
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
