import { useState, useRef, useEffect, useCallback } from 'react';
import {
  Folder,
  FolderOpen,
  Grid3X3,
  List,
  RefreshCw,
  Search,
  X,
} from 'lucide-react';
import { cn } from '@/lib/cn';
import { bridge, isDesktopBridge } from '@/lib/bridge';
import { useAssetStore, type AssetItem, type AssetMenuAction, type AssetTypeFilter } from '@/lib/assetStore';
import { useEditorStore } from '@/lib/editorStore';
import { useSpriteStore } from '@/lib/spriteStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useGenStore } from '@/lib/genStore';
import GenerateDialog from './GenerateDialog';
import CandidatesModal from './CandidatesModal';
import Thumb from './assetThumb';

const TYPE_FILTERS: Array<{ key: AssetTypeFilter; label: string }> = [
  { key: 'all', label: 'All' },
  { key: 'mesh', label: 'Mesh' },
  { key: 'model', label: '3D Model' },
  { key: 'texture', label: 'Texture' },
  { key: 'material', label: 'Material' },
  { key: 'sprite', label: 'Sprite' },
  { key: 'prefab', label: 'Prefab' },
  { key: 'scene', label: 'Scene' },
  { key: 'script', label: 'Script' },
  { key: 'audio', label: 'Audio' },
];

/** 标准目录排序(08 §2 项目布局),其余字母序兜底。 */
const FOLDER_ORDER = ['Meshes', 'Textures', 'Materials', 'Prefabs', 'Scenes', 'Scripts', 'Audio'];

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
      ? 'bg-info'
      : state === 'stale'
        ? 'bg-warn'
        : state === 'building'
          ? 'bg-info'
          : 'bg-sage';
  return (
    <span
      className={cn('absolute right-1 top-1 h-2 w-2 rounded-full', color)}
      title={`buildState: ${state ?? 'unknown'}`}
    />
  );
}

/** F10:点击选中 → selectedGuid(上下文 chip / 资产检视器同源)+ 右栏切「资产」页。 */
function useSelectAsset() {
  const setSelectedGuid = useAssetStore((s) => s.setSelectedGuid);
  const setRightTab = useWorkbenchStore((s) => s.setRightTab);
  return useCallback(
    (guid: string) => {
      setSelectedGuid(guid);
      setRightTab('asset');
    },
    [setSelectedGuid, setRightTab],
  );
}

function AssetGridItem({
  item,
  onMenu,
  onOpen,
}: {
  item: AssetItem;
  onMenu: (e: React.MouseEvent, item: AssetItem) => void;
  onOpen: (item: AssetItem) => void;
}) {
  const status = useAssetStore((s) => s.status[item.path]);
  const selected = useAssetStore((s) => s.selectedGuid === item.guid);
  const select = useSelectAsset();
  const tip = item.description ? `${item.path}\n${item.description}` : item.path;
  return (
    <div
      className={cn(
        'group relative flex flex-col items-center rounded-md border bg-shell-panel p-2 hover:bg-shell-hover',
        selected ? 'border-acc' : 'border-edge-strong',
      )}
      onClick={() => select(item.guid)}
      onDoubleClick={() => onOpen(item)}
      onContextMenu={(e) => onMenu(e, item)}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData('forge/asset-guid', item.guid);
        e.dataTransfer.setData('forge/asset-type', item.type);
        e.dataTransfer.setData('forge/asset-path', item.path);
        e.dataTransfer.setData(EDITOR_REFERENCE_MIME, JSON.stringify([makeAnnotation(editorReference('asset', { resourceId: item.guid, path: item.path }), item.path)]));
      }}
      data-asset-guid={item.guid}
      data-asset-path={item.path}
      data-selected={selected || undefined}
    >
      <BuildBadge state={status} />
      <AnnotationHandle reference={editorReference('asset', { resourceId: item.guid, path: item.path })} label={item.path} />
      <div className="h-10 w-10">
        <Thumb item={item} size={20} />
      </div>
      <span className="mt-1 max-w-full truncate text-2xs text-fg-2" title={tip}>
        {item.path.split('/').pop()}
      </span>
      <span className="text-2xs text-fg-4">{item.type}</span>
    </div>
  );
}

function AssetListItem({
  item,
  onMenu,
  onOpen,
}: {
  item: AssetItem;
  onMenu: (e: React.MouseEvent, item: AssetItem) => void;
  onOpen: (item: AssetItem) => void;
}) {
  const status = useAssetStore((s) => s.status[item.path]);
  const selected = useAssetStore((s) => s.selectedGuid === item.guid);
  const select = useSelectAsset();
  return (
    <div
      className={cn(
        'group relative flex items-center gap-2 rounded-md px-2 py-1 hover:bg-shell-hover',
        selected && 'bg-shell-active',
      )}
      onClick={() => select(item.guid)}
      onDoubleClick={() => onOpen(item)}
      onContextMenu={(e) => onMenu(e, item)}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData('forge/asset-guid', item.guid);
        e.dataTransfer.setData('forge/asset-type', item.type);
        e.dataTransfer.setData('forge/asset-path', item.path);
        e.dataTransfer.setData(EDITOR_REFERENCE_MIME, JSON.stringify([makeAnnotation(editorReference('asset', { resourceId: item.guid, path: item.path }), item.path)]));
      }}
      data-asset-guid={item.guid}
      data-asset-path={item.path}
      data-selected={selected || undefined}
    >
      <BuildBadge state={status} />
      <AnnotationHandle reference={editorReference('asset', { resourceId: item.guid, path: item.path })} label={item.path} />
      <div className="h-6 w-6">
        <Thumb item={item} size={14} />
      </div>
      <span className="min-w-0 flex-1 truncate text-xs text-fg-2">{item.path}</span>
      {item.description && (
        <span className="max-w-[260px] truncate text-2xs text-fg-4" title={item.description}>
          {item.description}
        </span>
      )}
      <span className="text-2xs text-fg-4">{item.type}</span>
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
  // 桌面能力:纯浏览器环境(无 Electron preload)→ 对应菜单项如实禁用(不伪造不可用功能)。
  // F8 wave.3:MOCK_FORGE_API 现提供诚实禁用实现(仍可调用、不抛异常),因此禁用判定
  // 以 isDesktopBridge() 为准——浏览器下两项菜单保持 disabled + 「仅桌面端可用」tooltip。
  const desktop = isDesktopBridge();
  const pickImport = desktop ? bridge().assets?.pickImport : undefined;
  const showInFolder = desktop ? bridge().assets?.showInFolder : undefined;

  // 工作区切换 → 资产面按新项目根重拉(asset_list 经 workspaceId 作用域)。
  const activeWorkspaceId = useWorkspaceStore((s) => s.activeWorkspaceId);
  useEffect(() => {
    load();
  }, [load, activeWorkspaceId]);

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
      return (
        i.path.toLowerCase().includes(q) ||
        i.guid.toLowerCase().includes(q) ||
        (i.description ?? '').toLowerCase().includes(q) ||
        (i.tags ?? []).some((t) => t.toLowerCase().includes(q))
      );
    }
    return true;
  });

  const onMenu = useCallback((e: React.MouseEvent, item: AssetItem) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, item });
  }, []);

  /** F-GAME-4:打开精灵编辑器 tab(texture = 先 sprite_create 建空精灵;sprite = 直开)。 */
  const openSpriteEditor = useCallback((item: AssetItem) => {
    const sprite = useSpriteStore.getState();
    if (item.type === 'sprite') void sprite.openSprite(item.path);
    else if (item.type === 'texture') void sprite.openFromTexture(item.path, item.guid);
    else return;
    useWorkbenchStore.getState().openTab('sprite-editor');
  }, []);

  /** 双击打开:.rxsprite → 精灵编辑器;.rxscene → 装进视口(切关卡/切场景的唯一 UI 入口)。 */
  const onOpenItem = useCallback(
    (item: AssetItem) => {
      if (item.type === 'sprite') openSpriteEditor(item);
      else if (item.type === 'scene') {
        // asset_list 路径以 Content/ 为根;scene_load 按项目根解析,须补前缀。
        const path = item.path.startsWith('Content/') ? item.path : `Content/${item.path}`;
        void useEditorStore.getState().openScenePath(path);
        useEditorStore.getState().setCenterTab('viewport');
      }
    },
    [openSpriteEditor],
  );

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
      case 'show-in-folder': {
        const ws = useWorkspaceStore.getState();
        const root = ws.workspaces.find((w) => w.id === ws.activeWorkspaceId)?.root;
        showInFolder?.(item.path, root);
        break;
      }
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
        // F10-RAG:绑定右键资产路径——其 .meta 简介+标签并入 gen_image 提示词。
        openGenDialog(currentFolder || 'Textures', item.path);
        break;
      case 'sprite-edit':
        openSpriteEditor(item);
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
  // F-GAME-4:按资产类型追加精灵入口(texture 建 .rxsprite;sprite 直开;其余不出现)。
  if (menu?.item.type === 'texture') {
    menuItems.push({ label: '编辑精灵(新建 .rxsprite)', action: 'sprite-edit' });
  } else if (menu?.item.type === 'sprite') {
    menuItems.push({ label: '编辑精灵', action: 'sprite-edit' });
  }

  const refsResult = store.refsResult;
  const pendingDelete = store.pendingDelete;

  return (
    <section className="flex min-h-0 flex-1 flex-col" aria-label="Assets">
      {/* 工具条(底栏波:标题/类型过滤/搜索/视图切换并作一行,190px 矮底栏省纵向空间;
          窄宽下搜索框整体换行,不把类型 chips 挤成 0 宽) */}
      <div className="flex shrink-0 flex-wrap items-center gap-x-2 gap-y-1 border-b border-edge px-2 py-1">
        <span className="shrink-0 text-2xs text-fg-4">Assets</span>

        <span className="flex min-w-[120px] flex-1 gap-1 overflow-x-auto">
          {TYPE_FILTERS.map((f) => (
            <button
              key={f.key}
              type="button"
              onClick={() => store.setTypeFilter(f.key)}
              className={cn(
                'shrink-0 rounded-full px-2 py-px text-2xs transition-colors',
                typeFilter === f.key ? 'bg-fg text-fg-inv' : 'bg-shell-panel text-fg-3 hover:text-fg-2',
              )}
            >
              {f.label}
            </button>
          ))}
        </span>

        <div className="flex w-[180px] min-w-[140px] items-center gap-1.5 rounded-md border border-edge-strong bg-shell-panel px-2 py-0.5">
          <Search size={12} className="shrink-0 text-fg-4" />
          <input
            value={search}
            onChange={(e) => store.setSearch(e.target.value)}
            placeholder="Search assets..."
            className="min-w-0 flex-1 bg-transparent text-xs text-fg outline-none placeholder:text-fg-4"
          />
        </div>

        <span className="flex shrink-0 items-center gap-0.5">
          <button type="button" title="Grid" className={cn(iconBtn, viewMode === 'grid' && 'bg-shell-active text-fg')} onClick={() => store.setViewMode('grid')}>
            <Grid3X3 size={11} strokeWidth={1.8} />
          </button>
          <button type="button" title="List" className={cn(iconBtn, viewMode === 'list' && 'bg-shell-active text-fg')} onClick={() => store.setViewMode('list')}>
            <List size={11} strokeWidth={1.8} />
          </button>
          <span className="h-3.5 w-px bg-edge-strong" />
          <button type="button" title="Refresh" className={iconBtn} onClick={() => load()}>
            <RefreshCw size={11} strokeWidth={1.8} />
          </button>
        </span>
      </div>

      {/* 删除提案条(非模态;模态 dialog 会永久阻塞无人值守冒烟,见 F1 坑) */}
      {pendingDelete && (
        <div className="mx-2 mt-1 shrink-0 rounded-md border border-edge-strong bg-shell-panel px-2 py-1" data-testid="delete-proposal">
          {pendingDelete.blocked === null ? (
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate text-2xs text-fg-2">
                删除 {pendingDelete.path}?源文件与 .meta 将一并删除。
              </span>
              <button
                type="button"
                className="shrink-0 rounded bg-fg px-2 py-px text-2xs text-fg-inv"
                onClick={() => void store.confirmDelete()}
              >
                确认删除
              </button>
              <button type="button" className="shrink-0 text-2xs text-fg-3" onClick={store.cancelDelete}>
                取消
              </button>
            </div>
          ) : (
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate text-2xs text-info" title={pendingDelete.blocked.join('\n')}>
                引用阻断,未删除。引用方:{pendingDelete.blocked.join('; ')}
              </span>
              <button type="button" className="shrink-0 text-2xs text-fg-3" onClick={store.cancelDelete}>
                关闭
              </button>
            </div>
          )}
        </div>
      )}

      {/* 主区:文件夹树左栏 + 资产内容 */}
      <div className="flex min-h-0 flex-1">
        {/* 文件夹树(07 §4 左栏;拖到文件夹 = asset_move 自动 redirector)
            —— 底栏窄宽时按比例收，不吃掉半条资产区 */}
        <div
          className="w-[38%] min-w-[84px] max-w-[132px] shrink-0 overflow-y-auto border-r border-edge py-1 pr-1"
          aria-label="资产文件夹"
        >
          <button
            type="button"
            onClick={() => store.setCurrentFolder('')}
            className={cn(
              'flex w-full items-center gap-1 rounded-r-md px-1.5 py-0.5 text-left text-2xs',
              currentFolder === '' ? 'bg-shell-active text-fg' : 'text-fg-3 hover:text-fg-2',
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
                currentFolder === f ? 'bg-shell-active text-fg' : 'text-fg-3 hover:text-fg-2',
                dragOverFolder === f && 'outline outline-1 outline-fg',
              )}
            >
              <Folder size={11} strokeWidth={1.8} />
              <span className="truncate">{f}</span>
            </button>
          ))}
        </div>

        {/* 资产列表(底栏横向铺开:网格按可用宽自动填列,不再钉死三列) */}
        <div className="min-w-0 flex-1 overflow-y-auto px-2 py-1">
          {loading && <p className="text-xs text-fg-4">Loading...</p>}
          {!loading && error && <p className="text-xs text-warn" title={error}>{error}</p>}
          {!loading && filtered.length === 0 && (
            <p className="text-xs text-fg-4">{items.length === 0 ? '暂无资产,先导入' : '无匹配资产'}</p>
          )}
          {viewMode === 'grid' ? (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(84px,1fr))] gap-1">
              {filtered.map((item) => (
                <AssetGridItem key={item.guid} item={item} onMenu={onMenu} onOpen={onOpenItem} />
              ))}
            </div>
          ) : (
            <div className="flex flex-col gap-0.5">
              {filtered.map((item) => (
                <AssetListItem key={item.guid} item={item} onMenu={onMenu} onOpen={onOpenItem} />
              ))}
            </div>
          )}
        </div>
      </div>

      {/* 引用查询浮层 */}
      {refsResult && (
        <div className="fixed bottom-4 right-4 z-50 flex w-72 flex-col rounded-md border border-edge-strong bg-shell-panel p-2 shadow-sm" data-testid="refs-panel">
          <div className="flex items-center justify-between">
            <span className="min-w-0 flex-1 truncate text-2xs font-medium text-fg" title={refsResult.path}>
              引用:{refsResult.path}
            </span>
            <button type="button" title="关闭" className={iconBtn} onClick={store.clearRefs}>
              <X size={11} strokeWidth={1.8} />
            </button>
          </div>
          <div className="mt-1 max-h-40 overflow-y-auto">
            <p className="text-2xs text-fg-4">被引用 (referencedBy):</p>
            {refsResult.referencedBy.length === 0 ? (
              <p className="pl-2 text-2xs text-fg-4">无</p>
            ) : (
              refsResult.referencedBy.map((r, i) => (
                <p key={i} className="truncate pl-2 text-2xs text-fg-2" title={r}>{r}</p>
              ))
            )}
            <p className="mt-1 text-2xs text-fg-4">引用 (refs):</p>
            {refsResult.refs.length === 0 ? (
              <p className="pl-2 text-2xs text-fg-4">无</p>
            ) : (
              refsResult.refs.map((r, i) => (
                <p key={i} className="truncate pl-2 text-2xs text-fg-2" title={r}>{r}</p>
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
          className="fixed z-50 flex w-48 flex-col rounded-md border border-edge-strong bg-shell-panel py-1 shadow-sm"
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
                a.disabled ? 'cursor-not-allowed text-fg-4' : 'text-fg-2 hover:bg-shell-hover',
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
  'flex h-6 w-6 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2';
import AnnotationHandle from './AnnotationHandle';
import { editorReference, EDITOR_REFERENCE_MIME, makeAnnotation } from '@/lib/editorReferences';
