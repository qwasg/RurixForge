import { ChevronLeft, ChevronRight, RotateCw, Star } from 'lucide-react';

const NAV_ICONS = [ChevronLeft, ChevronRight, RotateCw, Star];

/** Browser 视图:伪地址栏 + localhost:5173 拒连空态。 */
export default function BrowserView() {
  return (
    <div className="flex h-full flex-col">
      <div className="flex h-10 shrink-0 items-center gap-0.5 border-b border-line-soft px-2">
        {NAV_ICONS.map((Icon, i) => (
          <button
            key={i}
            type="button"
            className="rounded p-1.5 text-muted transition-colors hover:bg-panel-hover"
          >
            <Icon size={14} strokeWidth={1.75} />
          </button>
        ))}
        <div className="ml-1 flex-1 truncate rounded-md bg-panel px-2.5 py-1 text-xs text-muted">
          http://localhost:5173
        </div>
      </div>
      <div className="flex flex-1 flex-col items-center justify-center px-6 text-center">
        <p className="text-sm font-medium text-ink-soft">Can&apos;t connect to server</p>
        <p className="mt-1 text-xs text-muted">localhost:5173 refused to connect. (-102)</p>
        <div className="mt-4 flex items-center gap-2">
          <button type="button" className="btn-ghost">
            Ask Agent
          </button>
          <button type="button" className="btn-ghost">
            Show Details
          </button>
        </div>
      </div>
    </div>
  );
}
