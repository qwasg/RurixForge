import { useState, useRef, useEffect, useCallback } from 'react';
import {
  Box,
  FileAudio,
  FileCode,
  FileImage,
  Folder,
  FolderOpen,
  Grid3X3,
  LayoutGrid,
  List,
  Package,
  RefreshCw,
  Search,
  X,
} from 'lucide-react';
import { cn } from '@/lib/cn';
import { bridge } from '@/lib/bridge';
import { useAssetStore, type AssetItem, type AssetMenuAction, type AssetTypeFilter } from '@/lib/assetStore';
import { useGenStore } from '@/lib/genStore';
import GenerateDialog from './GenerateDialog';
import CandidatesModal from './CandidatesModal';

const TYPE_FILTERS: Array<{ key: AssetTypeFilter; label: string }> = [
  { key: 'all', label: 'All' },
  { key: 'mesh', label: 'Mesh' },
  { key: 'texture', label: 'Texture' },
  { key: 'material', label: 'Material' },
  { key: 'prefab', label: 'Prefab' },
  { key: 'scene', label: 'Scene' },
  { key: 'script', label: 'Script' },
  { key: 'audio', label: 'Audio' },
];

/** 标准目录排序(08 §2 项目布局),其余字母序兜底。 */
const FOLDER_ORDER = ['Meshes', 'Textures', 'Materials', 'Prefabs', 'Scenes', 'Scripts', 'Audio'];

function typeIcon(type: string, size: number) {
  switch (type) {
    case 'mesh':
      return <Box size={size} strokeWidth={1.5} />;
    case 'texture':
      return <FileImage size={size} strokeWidth={1.5} />;
    case 'material':
      return <Package size={size} strokeWidth={1.5} />;
    case 'prefab':
      return <LayoutGrid size={size} strokeWidth={1.5} />;
    case 'scene':
      return <FolderOpen size={size} strokeWidth={1.5} />;
    case 'script':
      return <FileCode size={size} strokeWidth={1.5} />;
    case 'audio':
      return <FileAudio size={size} strokeWidth={1.5} />;
    default:
      return <Package size={size} strokeWidth={1.5} />;
  }
}

/** 从资产路径推导一级文件夹(仅含真实有资产的目录,不虚构空目录)。 */
function deriveFolders(items: AssetItem[]): string[] {
  const set = new Set<string>();
  for (const i of items) {
    const seg = i.path.split('/')[0];
    if (seg && i.path.includes('/')) set.add(seg);
  }
  const rest = [...set].filter((f) => !FOLDER_ORDER.includes(f)).sort();
  return [...FOLDER_ORDER.filter((f) => set.has(f)), ...rest];
}

function BuildBadge({ state }: { state?: string }) {
  const color =
    state === 'failed'
      ? 'bg-accent-blue'
      : state === 'stale'
        ? 'bg-accent-yellow'
        : state === 'building'
          ? 'bg-accent-blue'
          : 'bg-accent-green';
  return (
    <span
      className={cn('absolute right-1 top-1 h-2 w-2 rounded-full', color)}
      title={`buildState: ${state ?? 'unknown'}`}
    />
  );
}

/** 缩略图:贴图原图直出;其他类型图标占位 + tooltip 如实标注(RD-F2-002)。 */
function Thumb({ item, size }: { item: AssetItem; size: number }) {
  const thumb = useAssetStore((s) => s.thumbs[item.guid]);
  const loadThumb = useAssetStore((s) => s.loadThumb);
  useEffect(() => {
    if (item.type === 'texture' && thumb === undefined) void loadThumb(item);
  }, [item, thumb, loadThumb]);

  if (item.type === 'texture' && thumb && thumb !== 'none') {
    return (
      <img
        src={thumb}
        alt={item.path}
        className="h-full w-full rounded-md object-cover"
        draggable={false}
      />
    );
  }
  const hint =
    item.type === 'texture'
      ? '缩略图不可用(源缺失或超 8MiB 上限)'
      : '缩略图未实现:网格三视角离屏渲染 = RD-F2-002';
  return (
    <div
      className="flex h-full w-full items-center justify-center rounded-md bg-panel text-muted"
      title={hint}
    >
      {typeIcon(item.type, size)}
    </div>
  );
}

function AssetGridItem({ item, onMenu }: { item: AssetItem; onMenu: (e: React.MouseEvent, item: AssetItem) => void }) {
  const status = useAssetStore((s) => s.status[item.path]);
  return (
    <div
      className="group relative flex flex-col items-center rounded-md border border-line bg-white p-2 hover:bg-panel-hover"
      onContextMenu={(e) => onMenu(e, item)}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData('forge/asset-guid', item.guid);
        e.dataTransfer.setData('forge/asset-type', item.type);
        e.dataTransfer.setData('forge/asset-path', item.path);
      }}
      data-asset-guid={item.guid}
      data-asset-path={item.path}
    >
      <BuildBadge state={status} />
      <div className="h-10 w-10">
        <Thumb item={item} size={20} />
      </div>
      <span className="mt-1 max-w-full truncate text-2xs text-ink-soft" title={item.path}>
        {item.path.split('/').pop()}
      </span>
      <span className="text-2xs text-muted-faint">{item.type}</span>
    </div>
  );
}

function AssetListItem({ item, onMenu }: { item: AssetItem; onMenu: (e: React.MouseEvent, item: AssetItem) => void }) {
  const status = useAssetStore((s) => s.status[item.path]);
  return (
    <div
      className="group relative flex items-center gap-2 rounded-md px-2 py-1 hover:bg-panel-hover"
      onContextMenu={(e) => onMenu(e, item)}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData('forge/asset-guid', item.guid);
        e.dataTransfer.setData('forge/asset-type', item.type);
        e.dataTransfer.setData('forge/asset-path', item.path);
      }}
      data-asset-guid={item.guid}
      data-asset-path={item.path}
    >
      <BuildBadge state={status} />
      <div className="h-6 w-6">
        <Thumb item={item} size={14} />
      </div>
      <span className="min-w-0 flex-1 truncate text-xs text-ink-soft">{item.path}</span>
      <span className="text-2xs text-muted-faint">{item.type}</span>
    </div>
  );
}

export default function AssetsPanel() {
  const store = useAssetStore();
  const { items, viewMode, typeFilter, search, currentFolder, loading, load, error } = store;
  const [menu, setMenu] = useState<{ x: number; y: number; item: AssetItem } | null>(null);
  const [dragOverFolder, setDragOverFolder] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const openGenDialog = useGenStore((s) => s.openDialog);
  // 桌面能力:web 端缺省 → 对应菜单项如实禁用(不伪造不可用功能)。
  const pickImport = bridge().assets?.pickImport;
  const showInFolder = bridge().assets?.showInFolder;

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    function onClick(e: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenu(null);
      }
    }
    if (menu) window.addEventListener('click', onClick);
    return () => window.removeEventListener('click', onClick);
  }, [menu]);

  const folders = deriveFolders(items);
  const filtered = items.filter((i) => {
    if (currentFolder !== '' && !i.path.startsWith(`${currentFolder}/`)) return false;
    if (typeFilter !== 'all' && i.type !== typeFilter) return false;
    if (search.trim()) {
      const q = search.toLowerCase();
      return i.path.toLowerCase().includes(q) || i.guid.toLowerCase().includes(q);
    }
    return true;
  });

  const onMenu = useCallback((e: React.MouseEvent, item: AssetItem) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, item });
  }, []);

  /** 拖到文件夹树节点 = 移动资产(asset_move 自动 redirector,07 §4)。 */
  const onDropFolder = (folder: string, e: React.DragEvent) => {
    e.preventDefault();
    setDragOverFolder(null);
    const path = e.dataTransfer.getData('forge/asset-path');
    if (!path || path.startsWith(`${folder}/`)) return;
    store.move(path, folder).catch(() => {});
  };

  const runAction = (action: AssetMenuAction) => {
    if (!menu) return;
    const { item } = menu;
    setMenu(null);
    switch (action) {
      case 'import-here':
        if (pickImport) {
          void pickImport().then((paths) => store.importToHere(paths)).catch(() => {});
        }
        break;
      case 'reimport':
        store.reimport(item.path).catch(() => {});
        break;
      case 'show-in-folder':
        showInFolder?.(item.path);
        break;
      case 'find-refs':
        store.queryRefs(item.path).catch(() => {});
        break;
      case 'delete-proposal':
        store.requestDelete(item.path);
        break;
      case 'gen-dialog':
        // F5 wave.3:直开生成对话框(destFolder = 当前 Assets 文件夹,全部 → Textures)。
        // 07 §4「生成(图像/模型,跳 Chat 预填)」契约更新为真实对话框;prefillChat seam
        // 仍保留在 editorStore(Chat 生成路径不受影响)。
        openGenDialog(currentFolder || 'Textures');
        break;
    }
  };

  const menuItems: Array<{ label: string; action: AssetMenuAction; disabled?: boolean; hint?: string }> = [
    {
      label: 'Import to here',
      action: 'import-here',
      disabled: !pickImport,
      hint: pickImport ? undefined : '仅桌面端可用(系统文件对话框)',
    },
    { label: 'Reimport', action: 'reimport' },
    {
      label: 'Show in folder',
      action: 'show-in-folder',
      disabled: !showInFolder,
      hint: showInFolder ? undefined : '仅桌面端可用(系统文件管理器)',
    },
    { label: 'Find refs', action: 'find-refs' },
    { label: 'Delete (Proposal)', action: 'delete-proposal' },
    { label: 'Generate...', action: 'gen-dialog' },
  ];

  const refsResult = store.refsResult;
  const pendingDelete = store.pendingDelete;

  return (
    <section className="flex h-[208px] shrink-0 flex-col border-t border-line-soft" aria-label="Assets">
      {/* 工具条 */}
      <div className="flex shrink-0 items-center justify-between px-2 pb-1 pt-2">
        <span className="text-2xs text-muted-faint">Assets</span>
        <span className="flex items-center gap-0.5">
          <button type="button" title="Grid" className={cn(iconBtn, viewMode === 'grid' && 'bg-panel-active text-ink')} onClick={() => store.setViewMode('grid')}>
            <Grid3X3 size={11} strokeWidth={1.8} />
          </button>
          <button type="button" title="List" className={cn(iconBtn, viewMode === 'list' && 'bg-panel-active text-ink')} onClick={() => store.setViewMode('list')}>
            <List size={11} strokeWidth={1.8} />
          </button>
          <span className="h-3.5 w-px bg-line" />
          <button type="button" title="Refresh" className={iconBtn} onClick={() => load()}>
            <RefreshCw size={11} strokeWidth={1.8} />
          </button>
        </span>
      </div>

      {/* 类型过滤 chips */}
      <div className="flex shrink-0 gap-1 overflow-x-auto px-2 pb-1">
        {TYPE_FILTERS.map((f) => (
          <button
            key={f.key}
            type="button"
            onClick={() => store.setTypeFilter(f.key)}
            className={cn(
              'shrink-0 rounded-full px-2 py-px text-2xs transition-colors',
              typeFilter === f.key ? 'bg-ink text-white' : 'bg-panel text-muted hover:text-ink-soft',
            )}
          >
            {f.label}
          </button>
        ))}
      </div>

      {/* 搜索 */}
      <div className="shrink-0 px-2 pb-1">
        <div className="flex items-center gap-1.5 rounded-md border border-line bg-white px-2 py-1">
          <Search size={12} className="shrink-0 text-muted-faint" />
          <input
            value={search}
            onChange={(e) => store.setSearch(e.target.value)}
            placeholder="Search assets..."
            className="min-w-0 flex-1 bg-transparent text-xs text-ink outline-none placeholder:text-muted-faint"
          />
        </div>
      </div>

      {/* 删除提案条(非模态;模态 dialog 会永久阻塞无人值守冒烟,见 F1 坑) */}
      {pendingDelete && (
        <div className="mx-2 mb-1 shrink-0 rounded-md border border-line bg-white px-2 py-1" data-testid="delete-proposal">
          {pendingDelete.blocked === null ? (
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate text-2xs text-ink-soft">
                删除 {pendingDelete.path}?源文件与 .meta 将一并删除。
              </span>
              <button
                type="button"
                className="shrink-0 rounded bg-ink px-2 py-px text-2xs text-white"
                onClick={() => void store.confirmDelete()}
              >
                确认删除
              </button>
              <button type="button" className="shrink-0 text-2xs text-muted" onClick={store.cancelDelete}>
                取消
              </button>
            </div>
          ) : (
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate text-2xs text-accent-blue" title={pendingDelete.blocked.join('\n')}>
                引用阻断,未删除。引用方:{pendingDelete.blocked.join('; ')}
              </span>
              <button type="button" className="shrink-0 text-2xs text-muted" onClick={store.cancelDelete}>
                关闭
              </button>
            </div>
          )}
        </div>
      )}

      {/* 主区:文件夹树左栏 + 资产内容 */}
      <div className="flex min-h-0 flex-1">
        {/* 文件夹树(07 §4 左栏;拖到文件夹 = asset_move 自动 redirector) */}
        <div className="w-[72px] shrink-0 overflow-y-auto border-r border-line-soft py-0.5 pr-1" aria-label="资产文件夹">
          <button
            type="button"
            onClick={() => store.setCurrentFolder('')}
            className={cn(
              'flex w-full items-center gap-1 rounded-r-md px-1.5 py-0.5 text-left text-2xs',
              currentFolder === '' ? 'bg-panel-active text-ink' : 'text-muted hover:text-ink-soft',
            )}
          >
            <FolderOpen size={11} strokeWidth={1.8} />
            <span className="truncate">全部</span>
          </button>
          {folders.map((f) => (
            <button
              key={f}
              type="button"
              onClick={() => store.setCurrentFolder(f)}
              onDragOver={(e) => {
                if (e.dataTransfer.types.includes('forge/asset-path')) {
                  e.preventDefault();
                  setDragOverFolder(f);
                }
              }}
              onDragLeave={() => setDragOverFolder((cur) => (cur === f ? null : cur))}
              onDrop={(e) => onDropFolder(f, e)}
              className={cn(
                'flex w-full items-center gap-1 rounded-r-md px-1.5 py-0.5 text-left text-2xs',
                currentFolder === f ? 'bg-panel-active text-ink' : 'text-muted hover:text-ink-soft',
                dragOverFolder === f && 'outline outline-1 outline-ink',
              )}
            >
              <Folder size={11} strokeWidth={1.8} />
              <span className="truncate">{f}</span>
            </button>
          ))}
        </div>

        {/* 资产列表 */}
        <div className="min-w-0 flex-1 overflow-y-auto px-2 pb-1">
          {loading && <p className="text-xs text-muted-faint">Loading...</p>}
          {!loading && error && <p className="text-xs text-amber-600" title={error}>{error}</p>}
          {!loading && filtered.length === 0 && (
            <p className="text-xs text-muted-faint">{items.length === 0 ? '暂无资产,先导入' : '无匹配资产'}</p>
          )}
          {viewMode === 'grid' ? (
            <div className="grid grid-cols-3 gap-1">
              {filtered.map((item) => (
                <AssetGridItem key={item.guid} item={item} onMenu={onMenu} />
              ))}
            </div>
          ) : (
            <div className="flex flex-col gap-0.5">
              {filtered.map((item) => (
                <AssetListItem key={item.guid} item={item} onMenu={onMenu} />
              ))}
            </div>
          )}
        </div>
      </div>

      {/* 引用查询浮层 */}
      {refsResult && (
        <div className="fixed bottom-4 right-4 z-50 flex w-72 flex-col rounded-md border border-line bg-white p-2 shadow-sm" data-testid="refs-panel">
          <div className="flex items-center justify-between">
            <span className="min-w-0 flex-1 truncate text-2xs font-medium text-ink" title={refsResult.path}>
              引用:{refsResult.path}
            </span>
            <button type="button" title="关闭" className={iconBtn} onClick={store.clearRefs}>
              <X size={11} strokeWidth={1.8} />
            </button>
          </div>
          <div className="mt-1 max-h-40 overflow-y-auto">
            <p className="text-2xs text-muted-faint">被引用 (referencedBy):</p>
            {refsResult.referencedBy.length === 0 ? (
              <p className="pl-2 text-2xs text-muted-faint">无</p>
            ) : (
              refsResult.referencedBy.map((r, i) => (
                <p key={i} className="truncate pl-2 text-2xs text-ink-soft" title={r}>{r}</p>
              ))
            )}
            <p className="mt-1 text-2xs text-muted-faint">引用 (refs):</p>
            {refsResult.refs.length === 0 ? (
              <p className="pl-2 text-2xs text-muted-faint">无</p>
            ) : (
              refsResult.refs.map((r, i) => (
                <p key={i} className="truncate pl-2 text-2xs text-ink-soft" title={r}>{r}</p>
              ))
            )}
          </div>
        </div>
      )}

      {/* F5 wave.3:生成对话框 + 候选挑拣 modal(右键「Generate...」链路) */}
      <GenerateDialog />
      <CandidatesModal />

      {/* 右键菜单(固定六项,07 §4) */}
      {menu && (
        <div
          ref={menuRef}
          className="fixed z-50 flex w-48 flex-col rounded-md border border-line bg-white py-1 shadow-sm"
          style={{ left: menu.x, top: menu.y }}
        >
          {menuItems.map((a) => (
            <button
              key={a.action}
              type="button"
              disabled={a.disabled}
              title={a.hint}
              className={cn(
                'px-3 py-1 text-left text-xs',
                a.disabled ? 'cursor-not-allowed text-muted-faint' : 'text-ink-soft hover:bg-panel-hover',
              )}
              onClick={() => runAction(a.action)}
            >
              {a.label}
            </button>
          ))}
        </div>
      )}
    </section>
  );
}

const iconBtn =
  'flex h-6 w-6 items-center justify-center rounded-md text-muted transition-colors hover:bg-panel-hover hover:text-ink-soft';
