import { ChevronDown, GitBranch } from 'lucide-react';
import { AGENT_BRANCH } from '@/lib/mock';

/** Changes 视图:提交/分支头部 + 空态(diff 默认隐藏)。 */
export default function ChangesView() {
  return (
    <div className="flex h-full flex-col">
      <div className="flex h-10 shrink-0 items-center gap-1 border-b border-line-soft px-2">
        <button
          type="button"
          className="flex items-center gap-1.5 rounded px-1.5 py-1 text-xs text-ink-soft transition-colors hover:bg-panel-hover"
        >
          <GitBranch size={13} className="text-muted" />
          <span>All Commits</span>
          <ChevronDown size={12} className="text-muted-faint" />
        </button>
        <button
          type="button"
          className="flex items-center gap-1 rounded px-1.5 py-1 text-xs transition-colors hover:bg-panel-hover"
        >
          <span className="text-accent-green">+168427</span>
          <span className="text-red-600">-169</span>
          <ChevronDown size={12} className="text-muted-faint" />
        </button>
        <button
          type="button"
          className="flex min-w-0 items-center gap-1 rounded px-1.5 py-1 text-xs text-muted transition-colors hover:bg-panel-hover"
        >
          <span className="truncate">{AGENT_BRANCH}</span>
          <ChevronDown size={12} className="shrink-0 text-muted-faint" />
        </button>
        <div className="flex-1" />
        <button type="button" className="btn-primary shrink-0">
          Commit &amp; Push
        </button>
      </div>
      <div className="flex flex-1 flex-col items-center justify-center px-6 text-center">
        <p className="text-xs text-muted">Open a file to get started.</p>
        <p className="mt-1 text-xs text-muted-faint">Large diffs are hidden by default.</p>
        <div className="mt-4 flex items-center gap-2">
          <button type="button" className="btn-ghost">
            New File
          </button>
          <button type="button" className="btn-ghost">
            Load Diff
          </button>
        </div>
      </div>
    </div>
  );
}
