import { FolderTree, ListTree, Package, SlidersHorizontal } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useWorkbenchStore, type RightTab } from '@/lib/workbenchStore';
import AssetInspectorPanel from '@/components/editor/AssetInspectorPanel';
import EntityInspectorPanel from '@/components/editor/EntityInspectorPanel';
import HierarchyPanel from '@/components/editor/HierarchyPanel';
import Inspector from './Inspector';
import { PaneToggleBtn } from './primitives';

/**
 * 壳右栏(2026-08-24):「工作区」文件树 / 「层级」场景树 / 「属性」实体检视 /
 * 「资产」素材简介检视(F10)四选一。
 * 编辑器 tab 激活时层级接管本栏(切换逻辑在 workbenchStore 的 tab action 里,
 * 与激活 tab 同事务;手动切换在下次 tab 变更前保持;资产面板点击选中自动切「资产」页)。
 */

const TABS: Array<{ id: RightTab; label: string; icon: typeof FolderTree }> = [
  { id: 'files', label: '工作区', icon: FolderTree },
  { id: 'hierarchy', label: '层级', icon: ListTree },
  { id: 'properties', label: '属性', icon: SlidersHorizontal },
  { id: 'asset', label: '资产', icon: Package },
];

export default function RightPane() {
  const tab = useWorkbenchStore((st) => st.rightTab);
  const setRightTab = useWorkbenchStore((st) => st.setRightTab);

  return (
    <div data-testid="right-pane" className="flex h-full min-h-0 flex-col bg-shell-sidebar">
      <div className="flex shrink-0 flex-col gap-1 px-2.5 pb-1.5 pt-2">
        <span className="flex items-center gap-0.5 rounded-md bg-shell-sunk p-0.5">
          {TABS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              data-testid={`rightpane-tab-${id}`}
              onClick={() => setRightTab(id)}
              className={cn(
                'flex items-center gap-1 rounded px-2 py-0.5 text-[11px] transition-colors',
                tab === id ? 'bg-shell-panel text-fg shadow-sm' : 'text-fg-3 hover:text-fg-2',
              )}
            >
              <Icon size={11} />
              {label}
            </button>
          ))}
        </span>
        <PaneToggleBtn kind="inspector" className="h-7 w-7" />
      </div>
      <div className="flex min-h-0 flex-1 flex-col">
        {tab === 'files' ? (
          <Inspector />
        ) : tab === 'hierarchy' ? (
          <HierarchyPanel />
        ) : tab === 'asset' ? (
          <AssetInspectorPanel />
        ) : (
          <EntityInspectorPanel />
        )}
      </div>
    </div>
  );
}
