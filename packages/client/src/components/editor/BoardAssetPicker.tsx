import { useEffect, useRef, useState } from 'react';
import { FolderInput, RefreshCw, Search } from 'lucide-react';
import { cn } from '@/lib/cn';
import { bridge, isDesktopBridge } from '@/lib/bridge';
import { useAssetStore, type AssetItem } from '@/lib/assetStore';
import { useDesignBoardStore } from '@/lib/designBoardStore';
import { useToastStore } from '@/lib/toastStore';
import Thumb from './assetThumb';

/**
 * 画板素材选择器(实体卡「+ 素材」与详情画布「添加素材」共用的锚定弹层):
 * 资产列表来自 useAssetStore(asset_list;为空时自动拉取,离线如实报错),
 * 支持搜索 + 类型过滤 + 缩略图网格;点击挂载到实体(按 guid 去重,已挂载置灰);
 * 底部「导入文件…」桌面端走系统对话框(pickImport + importToHere),web 端如实禁用。
 * 定位交给调用方(className 传 absolute 定位类)。
 */

const FILTERS: Array<{ id: string; label: string }> = [
  { id: 'all', label: '全部' },
  { id: 'texture', label: '贴图' },
  { id: 'mesh', label: '网格' },
  { id: 'material', label: '材质' },
  { id: 'prefab', label: '预制' },
  { id: 'audio', label: '音频' },
];

export interface BoardAssetPickerProps {
  nodeId: string;
  onClose: () => void;
  /** 定位类(absolute + 方位),由调用方按锚点给 */
  className?: string;
}

export default function BoardAssetPicker({ nodeId, onClose, className }: BoardAssetPickerProps) {
  const items = useAssetStore((s) => s.items);
  const loading = useAssetStore((s) => s.loading);
  const error = useAssetStore((s) => s.error);
  const load = useAssetStore((s) => s.load);
  const importToHere = useAssetStore((s) => s.importToHere);
  const node = useDesignBoardStore((s) => s.nodes.find((n) => n.id === nodeId));
  const attachAsset = useDesignBoardStore((s) => s.attachAsset);
  const pushToast = useToastStore((s) => s.push);

  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [importing, setImporting] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  // 点外部关闭(捕获期,避免与卡身拖拽/画布平移抢事件)
  useEffect(() => {
    const onDown = (ev: MouseEvent) => {
      if (!ref.current?.contains(ev.target as Node)) onClose();
    };
    document.addEventListener('mousedown', onDown, true);
    return () => document.removeEventListener('mousedown', onDown, true);
  }, [onClose]);

  // 首开自动拉列表(仅尝试一次,失败如实显示 error,不打转)
  const triedRef = useRef(false);
  useEffect(() => {
    if (triedRef.current) return;
    triedRef.current = true;
    if (items.length === 0 && !loading) void load();
  }, [items.length, loading, load]);

  const attached = new Set((node?.assets ?? []).map((a) => a.guid));
  const q = query.trim().toLowerCase();
  const visible = items.filter(
    (it) =>
      (filter === 'all' || it.type === filter) &&
      (q === '' || it.path.toLowerCase().includes(q)),
  );

  const desktop = isDesktopBridge();
  const pickImport = desktop ? bridge().assets?.pickImport : undefined;

  const onImport = () => {
    if (!pickImport || importing) return;
    setImporting(true);
    pickImport()
      .then((paths) => importToHere(paths))
      .catch((err: unknown) => {
        pushToast('error', `导入失败: ${(err as Error).message}`);
      })
      .finally(() => setImporting(false));
  };

  return (
    <div
      ref={ref}
      data-no-drag
      data-testid={`board-asset-picker-${nodeId}`}
      className={cn(
        'z-30 w-[268px] rounded-md border border-edge-strong bg-shell-float p-1.5 shadow-pop',
        className,
      )}
    >
      <div className="flex items-center gap-1 pb-1">
        <Search size={11} strokeWidth={1.8} className="shrink-0 text-fg-4" />
        <input
          autoFocus
          data-testid={`board-asset-search-${nodeId}`}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') onClose();
          }}
          placeholder="搜索资产路径…"
          className="min-w-0 flex-1 bg-transparent text-2xs text-fg outline-none placeholder:text-fg-4"
        />
        <button
          type="button"
          data-testid={`board-asset-reload-${nodeId}`}
          title="刷新资产列表"
          disabled={loading}
          onClick={() => void load()}
          className="shrink-0 rounded p-0.5 text-fg-4 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:opacity-40"
        >
          <RefreshCw size={11} strokeWidth={1.8} className={loading ? 'animate-spin' : undefined} />
        </button>
      </div>

      <div className="flex flex-wrap gap-0.5 pb-1">
        {FILTERS.map((f) => (
          <button
            key={f.id}
            type="button"
            data-testid={`board-asset-filter-${f.id}`}
            onClick={() => setFilter(f.id)}
            className={cn(
              'rounded px-1.5 py-px text-[10px] transition-colors',
              filter === f.id
                ? 'bg-shell-active text-fg'
                : 'text-fg-4 hover:bg-shell-hover hover:text-fg-2',
            )}
          >
            {f.label}
          </button>
        ))}
      </div>

      {error !== null && (
        <p className="px-1 pb-1 text-[10px] text-danger" data-testid={`board-asset-error-${nodeId}`}>
          {error}
        </p>
      )}

      {loading ? (
        <p className="px-1 py-2 text-center text-[10px] text-fg-4">资产列表加载中…</p>
      ) : visible.length === 0 ? (
        <p className="px-1 py-2 text-center text-[10px] text-fg-4">
          {items.length === 0 ? '工程暂无资产(可从下方导入)' : '没有匹配的资产'}
        </p>
      ) : (
        <div className="grid max-h-[216px] grid-cols-3 gap-1 overflow-y-auto">
          {visible.map((it) => {
            const owned = attached.has(it.guid);
            const base = it.path.split('/').pop() ?? it.path;
            return (
              <button
                key={it.guid}
                type="button"
                data-testid={`board-asset-opt-${nodeId}-${it.guid}`}
                disabled={owned}
                title={owned ? `${it.path}(已挂载)` : `${it.path} [${it.type}],点击挂载`}
                onClick={() => attachAsset(nodeId, { guid: it.guid, path: it.path, type: it.type })}
                className={cn(
                  'group flex flex-col gap-0.5 rounded-md border border-edge p-1 text-left transition-colors',
                  owned ? 'opacity-40' : 'hover:border-edge-strong hover:bg-shell-hover',
                )}
              >
                <span className="block h-12 w-full overflow-hidden rounded-md">
                  <Thumb item={it as AssetItem} size={18} />
                </span>
                <span className="w-full truncate text-[10px] text-fg-2">{base}</span>
                <span className="w-full truncate text-[9px] text-fg-4">
                  {it.type}
                  {owned ? ' · 已挂' : ''}
                </span>
              </button>
            );
          })}
        </div>
      )}

      <div className="mt-1 flex items-center gap-1 border-t border-edge pt-1">
        <button
          type="button"
          data-testid={`board-asset-import-${nodeId}`}
          disabled={!pickImport || importing}
          title={pickImport ? '系统文件对话框选择源文件导入' : '仅桌面端可用(系统文件对话框)'}
          onClick={onImport}
          className="flex items-center gap-1 rounded border border-edge-strong px-1.5 py-0.5 text-[10px] text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:opacity-40 disabled:hover:bg-transparent"
        >
          <FolderInput size={10} strokeWidth={1.8} />
          {importing ? '导入中…' : '导入文件…'}
        </button>
        <span className="flex-1" />
        <span className="text-[9px] text-fg-4">{items.length} 项资产</span>
      </div>
    </div>
  );
}
