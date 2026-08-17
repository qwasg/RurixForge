import { FileText, Globe, Terminal, Upload } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useAppStore } from '@/lib/store';
import type { RightPanelView } from '@/lib/store';

interface MenuRow {
  view: Exclude<RightPanelView, 'menu'>;
  label: string;
  icon: LucideIcon;
  badge?: string;
}

const ROWS: MenuRow[] = [
  { view: 'changes', label: 'Changes', icon: Upload, badge: '+890976' },
  { view: 'browser', label: 'Browser', icon: Globe },
  { view: 'terminal', label: 'Terminal', icon: Terminal },
  { view: 'files', label: 'Files', icon: FileText },
];

/** 窄栏菜单:灰字 'On rurix' + Changes/Browser/Terminal/Files 四行。 */
export default function PanelMenu() {
  const setRightPanelView = useAppStore((s) => s.setRightPanelView);

  return (
    <div className="flex h-full flex-col px-2 pt-3">
      <div className="px-2 pb-1 text-xs text-muted">On rurix</div>
      {ROWS.map((row) => (
        <button key={row.view} type="button" onClick={() => setRightPanelView(row.view)} className="nav-row">
          <row.icon size={15} strokeWidth={1.75} className="shrink-0 text-muted" />
          <span>{row.label}</span>
          {row.badge && <span className="ml-auto text-xs text-accent-green">{row.badge}</span>}
        </button>
      ))}
    </div>
  );
}
