import { FileCode2, GitCompare, ListChecks, ListTree, Sparkles, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { runCommand } from '@/lib/commands';
import { useWorkbenchStore, type TabKind } from '@/lib/workbenchStore';
import EditorView from '@/views/EditorView';
import PlanTab from '@/components/workbench/PlanTab';
import ProposalsTab from '@/components/workbench/ProposalsTab';
import TodoTab from '@/components/workbench/TodoTab';

/**
 * F7 wave.3/5 Workbench(参考 ui/workbench.rs):
 * tabbar 34px(bg-sunk;激活 tab 顶 2px accent 条 + icon + 标题 + 16×16 关闭);
 * 内建 tab:编辑器(游戏原生 EditorView 嵌入,内部零改动)/ Plan / Todo / 提案(wave.5);
 * 空态(无 tab)= 居中卡「个人工作区」+ 胶囊 tip(新建会话/打开编辑器)。
 *
 * 差异留痕(wave.5):参考 diff tab 为代码 diff 页——本仓 proposals 是 F2 治理确认单
 * (无代码内容 diff 数据面),诚实适配为「提案」tab;双行号 gutter diff 渲染器留 RD-F7-004。
 */

const TAB_ICONS: Record<TabKind, typeof FileCode2> = {
  editor: FileCode2,
  plan: ListTree,
  todo: ListChecks,
  proposals: GitCompare,
};

export default function Workbench() {
  const tabs = useWorkbenchStore((st) => st.tabs);
  const activeTabId = useWorkbenchStore((st) => st.activeTabId);
  const activateTab = useWorkbenchStore((st) => st.activateTab);
  const closeTab = useWorkbenchStore((st) => st.closeTab);

  const active = tabs.find((t) => t.id === activeTabId) ?? null;

  return (
    <div data-testid="workbench" className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-shell-bg">
      {/* tabbar 34 */}
      <div className="flex h-[34px] shrink-0 items-end border-b border-edge bg-shell-sunk">
        {tabs.map((t) => {
          const isActive = t.id === activeTabId;
          const TabIcon = TAB_ICONS[t.kind] ?? FileCode2;
          return (
            <div
              key={t.id}
              role="button"
              tabIndex={0}
              data-testid={`workbench-tab-${t.id}`}
              onClick={() => activateTab(t.id)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') activateTab(t.id);
              }}
              className={cn(
                'flex h-[34px] items-center gap-1.5 border-t-2 pl-3 pr-1.5 text-[12px]',
                isActive ? 'border-acc bg-shell-bg text-fg' : 'border-transparent text-fg-3',
              )}
              style={{ borderTopColor: isActive ? 'var(--accent)' : 'transparent' }}
            >
              <TabIcon size={12} className={isActive ? 'text-fg-2' : 'text-fg-4'} />
              <span className="max-w-[140px] truncate">{t.title}</span>
              <button
                type="button"
                title="关闭"
                aria-label={`关闭 ${t.title}`}
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(t.id);
                }}
                className="flex h-4 w-4 items-center justify-center rounded text-fg-3 transition-colors hover:bg-shell-hover"
              >
                <X size={10} />
              </button>
            </div>
          );
        })}
        <span className="min-w-[40px] flex-1" />
      </div>

      {/* 内容 */}
      <div className="min-h-0 min-w-0 flex-1">
        {active?.kind === 'editor' ? (
          <EditorView />
        ) : active?.kind === 'plan' ? (
          <PlanTab />
        ) : active?.kind === 'todo' ? (
          <TodoTab />
        ) : active?.kind === 'proposals' ? (
          <ProposalsTab />
        ) : (
          <div className="flex h-full items-center justify-center p-8">
            <div className="flex w-[420px] max-w-full flex-col gap-2.5 rounded-xl border border-edge bg-shell-panel p-7 shadow-sh1">
              <span className="font-serif text-[28px] font-semibold text-fg">个人工作区</span>
              <span className="text-[13px] text-fg-2">
                从左侧选择会话，或打开编辑器后开始搭建场景。
              </span>
              <span className="mt-1.5 flex flex-wrap gap-2">
                <button
                  type="button"
                  data-testid="empty-new-session"
                  onClick={() => runCommand('session.new')}
                  className="flex h-6 items-center gap-1 rounded-full border border-edge bg-shell-bg px-2.5 text-[11px] text-fg-3 transition-colors hover:bg-shell-hover"
                >
                  <Sparkles size={11} />
                  新建会话
                </button>
                <button
                  type="button"
                  data-testid="empty-open-editor"
                  onClick={() => runCommand('tab.editor')}
                  className="flex h-6 items-center gap-1 rounded-full border border-edge bg-shell-bg px-2.5 text-[11px] text-fg-3 transition-colors hover:bg-shell-hover"
                >
                  <FileCode2 size={11} />
                  打开编辑器
                </button>
              </span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
