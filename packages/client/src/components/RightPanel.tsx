import { ChevronLeft, X } from 'lucide-react';
import { useAppStore } from '@/lib/store';
import type { RightPanelView } from '@/lib/store';
import PanelMenu from './panel/PanelMenu';
import ChangesView from './panel/ChangesView';
import BrowserView from './panel/BrowserView';
import TerminalView from './panel/TerminalView';
import FilesView from './panel/FilesView';

const VIEW_TITLES: Record<Exclude<RightPanelView, 'menu'>, string> = {
  changes: 'Changes',
  browser: 'Browser',
  terminal: 'Terminal',
  files: 'Files',
};

/**
 * 右侧面板:'menu' 为窄栏(On rurix 四行入口),
 * 其余视图为宽栏(顶部 ‹ 视图名 ✕ 小 tab 条 + 对应内容)。
 */
export default function RightPanel() {
  const open = useAppStore((s) => s.rightPanelOpen);
  const view = useAppStore((s) => s.rightPanelView);
  const setRightPanelView = useAppStore((s) => s.setRightPanelView);

  if (!open) return null;

  if (view === 'menu') {
    return (
      <aside className="flex h-full w-[230px] shrink-0 flex-col border-l border-line-soft bg-white">
        <PanelMenu />
      </aside>
    );
  }

  return (
    <aside className="flex h-full w-[420px] shrink-0 flex-col border-l border-line-soft bg-white">
      <div className="flex h-9 shrink-0 items-center gap-1 border-b border-line-soft px-2">
        <button
          type="button"
          onClick={() => setRightPanelView('menu')}
          className="rounded p-1 text-muted transition-colors hover:bg-panel-hover"
          aria-label="Back"
        >
          <ChevronLeft size={15} />
        </button>
        <span className="text-xs font-medium text-ink">{VIEW_TITLES[view]}</span>
        <div className="flex-1" />
        <button
          type="button"
          onClick={() => setRightPanelView('menu')}
          className="rounded p-1 text-muted transition-colors hover:bg-panel-hover"
          aria-label="Close"
        >
          <X size={14} />
        </button>
      </div>
      <div className="min-h-0 flex-1">
        {view === 'changes' && <ChangesView />}
        {view === 'browser' && <BrowserView />}
        {view === 'terminal' && <TerminalView />}
        {view === 'files' && <FilesView />}
      </div>
    </aside>
  );
}
