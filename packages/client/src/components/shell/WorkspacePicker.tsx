import { useEffect, useMemo, useState, type ReactNode } from 'react';
import {
  Boxes,
  Check,
  ChevronDown,
  ChevronRight,
  Folder,
  FolderPlus,
  HardDrive,
  Plus,
  Search,
  Trash2,
} from 'lucide-react';
import { bridge, isDesktopBridge } from '@/lib/bridge';
import { cn } from '@/lib/cn';
import { apiPost, ForgeApiError } from '@/lib/forgeApi';
import { useToastStore } from '@/lib/toastStore';
import { displayRoot, useWorkspaceStore, type ForgeWorkspace } from '@/lib/workspaceStore';

/** F-GAME-3:新建工作区时的游戏选型(制作前定 2D/3D,写入 forge.toml [project] mode) */
type GameTypeChoice = '2d' | '3d' | 'none';
type GameBackendChoice = 'godot' | 'rurix';

/**
 * 工作区选择器(2026-08-25 用户拍板:WORKSPACES 由会话列上方搬到底部用户卡上方)。
 *
 * 收起态 = 一条 sec-head 触发条贴在用户卡之上;点开后面板落在触发条与用户卡之间,
 * 触发条被面板顶着上移——「上移」是 flex 自然结果(会话列 flex-1 让出高度),不是位移动画。
 * 面板封顶 60% 侧栏高,内部自滚,保证会话列不被挤没。
 * 内容参考 Cursor 工作区弹层:搜索 + 最近 + 打开动作三段。
 *
 * 诚实禁用面(D-021 纪律,不伪造):「本机目录…」依赖 Electron 系统目录对话框,
 * 纯浏览器下禁用并给 tooltip。D-040:原恒禁用的「云端 · 未接入」行无任何后端语义,下线。
 * 展开状态在 workspaceStore.pickerOpen,状态栏工作区段也能直接唤起本面板。
 */

/** 面板内 10px 段落头(与 primitives.SecHead 同型,缩进对齐面板行) */
function PickerSecHead({ label }: { label: string }) {
  return (
    <div className="px-2.5 pb-0.5 pt-1.5 text-[10px] font-semibold tracking-wide text-fg-4">
      {label}
    </div>
  );
}

/** 28px 动作行(icon + 文案 + 可选 hint/选中勾/次级箭头) */
function PickerRow({
  icon,
  label,
  hint,
  chevron,
  selected,
  disabled,
  title,
  testId,
  onSelect,
}: {
  icon: ReactNode;
  label: string;
  hint?: string;
  chevron?: boolean;
  selected?: boolean;
  disabled?: boolean;
  title?: string;
  testId: string;
  onSelect?: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      title={title}
      data-testid={testId}
      onClick={onSelect}
      className={cn(
        'flex h-7 w-full items-center gap-2 rounded-md px-2.5 text-left text-[12px] text-fg-2 transition-colors hover:bg-shell-hover disabled:opacity-40 disabled:hover:bg-transparent',
        selected === true && 'bg-shell-active',
      )}
    >
      <span className="flex h-[13px] w-[13px] shrink-0 items-center justify-center text-fg-3">
        {icon}
      </span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {hint !== undefined && <span className="shrink-0 text-[10px] text-fg-4">{hint}</span>}
      {selected === true && <Check size={11} className="shrink-0 text-acc" />}
      {chevron === true && <ChevronRight size={11} className="shrink-0 text-fg-4" />}
    </button>
  );
}

export default function WorkspacePicker() {
  const workspaces = useWorkspaceStore((st) => st.workspaces);
  const recentIds = useWorkspaceStore((st) => st.recentIds);
  const activeWorkspaceId = useWorkspaceStore((st) => st.activeWorkspaceId);
  const createWorkspace = useWorkspaceStore((st) => st.create);
  const removeWorkspace = useWorkspaceStore((st) => st.remove);
  const setActiveWorkspace = useWorkspaceStore((st) => st.setActive);
  const pushToast = useToastStore((st) => st.push);
  const open = useWorkspaceStore((st) => st.pickerOpen);
  const setOpen = useWorkspaceStore((st) => st.setPickerOpen);

  const [query, setQuery] = useState('');
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');
  const [root, setRoot] = useState('');
  const [picking, setPicking] = useState(false);
  /** F-GAME-3:游戏类型选择(默认 2D;选 none = 仅登记目录,不初始化项目) */
  const [gameType, setGameType] = useState<GameTypeChoice>('2d');
  const [backend3d, setBackend3d] = useState<GameBackendChoice>('rurix');
  const [submitting, setSubmitting] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  const pickFolder = isDesktopBridge() ? bridge().workspace?.pickFolder : undefined;

  /** 最近序在前(新→旧),其余按 updatedAt 兜底 */
  const ordered = useMemo<ForgeWorkspace[]>(() => {
    const rest = new Map(workspaces.map((w) => [w.id, w] as const));
    const head: ForgeWorkspace[] = [];
    for (const id of recentIds) {
      const w = rest.get(id);
      if (w !== undefined) {
        head.push(w);
        rest.delete(id);
      }
    }
    const tail = [...rest.values()].sort((a, b) => (a.updatedAt < b.updatedAt ? 1 : -1));
    return [...head, ...tail];
  }, [workspaces, recentIds]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (q === '') return ordered;
    return ordered.filter(
      (w) => w.name.toLowerCase().includes(q) || w.root.toLowerCase().includes(q),
    );
  }, [ordered, query]);

  const resetForm = () => {
    setName('');
    setRoot('');
    setGameType('2d');
    setBackend3d('rurix');
    setCreateError(null);
  };

  const openPanel = (withForm: boolean) => {
    if (submitting) return;
    setOpen(true);
    setQuery('');
    setCreating(withForm);
    resetForm();
  };

  const closePanel = () => {
    if (submitting) return;
    setOpen(false);
    setCreating(false);
    resetForm();
  };

  // 外部唤起(状态栏工作区段)时清空上次的搜索词
  useEffect(() => {
    if (open) setQuery('');
  }, [open]);

  const selectWorkspace = (id: string | null) => {
    if (submitting) return;
    setActiveWorkspace(id);
    closePanel();
  };

  const submitWorkspace = async () => {
    const n = name.trim();
    const r = root.trim();
    if (n === '' || submitting) return;
    const gt = gameType;
    const backend = gt === '2d' ? 'godot' : backend3d;
    setSubmitting(true);
    setCreateError(null);
    let projectRoot = r;
    try {
      // F-GAME-3:选了游戏类型 → 先 project/init 落定模式(forge.toml + 起始场景,
      // 留空时由后端分配目录),再登记返回的同一目录;仅目录不初始化项目。
      if (gt !== 'none') {
        try {
          const result = await apiPost<{ project: { root: string } }>('/api/forge/project/init', {
            root: r, name: n, mode: gt, backend,
          });
          projectRoot = result.project.root || r;
          if (projectRoot === '') throw new Error('项目初始化未返回目录，请重试');
          // 初始化已成功但登记失败时，重试沿用这次创建的目录。
          if (r === '') setRoot(displayRoot(projectRoot));
        } catch (err) {
          // 目录已是项目(含 forge.toml)→ 保留其既有模式,继续登记工作区。
          if (err instanceof ForgeApiError && err.code === 'PROJECT_ALREADY_INITIALIZED') {
            pushToast('info', `「${n}」已是游戏项目,沿用其既有模式与后端登记`);
          } else {
            throw err;
          }
        }
      }
      const workspace = await createWorkspace(n, projectRoot, { createRoot: true });
      if (workspace === null) {
        setCreateError('工作区创建失败，请检查错误提示后重试。');
        return;
      }
      setCreating(false);
      resetForm();
      pushToast('success', `已创建工作区「${n}」`);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setCreateError(message);
      pushToast('error', `工作区创建失败:${message}`);
    } finally {
      setSubmitting(false);
    }
  };

  /** Esc 先收表单再轮到面板,所以这里吃掉冒泡 */
  const onFormKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (submitting) return;
    if (e.key === 'Enter') void submitWorkspace();
    if (e.key === 'Escape') {
      e.stopPropagation();
      setCreating(false);
      resetForm();
    }
  };

  /** 桌面端系统目录对话框:目录名兜底作工作区名,选完直接建并切过去 */
  const onPickFolder = () => {
    if (pickFolder === undefined || picking || submitting) return;
    setPicking(true);
    pickFolder()
      .then(async (dir) => {
        if (dir === null || dir === '') return;
        const base = dir.split(/[\\/]/).filter((s) => s !== '').pop() ?? dir;
        const ws = await createWorkspace(base, dir);
        if (ws !== null) closePanel();
      })
      .catch((err: unknown) => {
        pushToast('error', `选择目录失败:${(err as Error).message}`);
      })
      .finally(() => setPicking(false));
  };

  return (
    <div className="flex max-h-[60%] shrink-0 flex-col border-t border-edge">
      {/* 触发条:保持原 WORKSPACES sec-head 形状,整条可点 */}
      <div
        role="button"
        tabIndex={0}
        aria-expanded={open}
        data-testid="workspace-picker-toggle"
        onClick={() => (open ? closePanel() : openPanel(false))}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            if (open) closePanel();
            else openPanel(false);
          }
          if (e.key === 'Escape') closePanel();
        }}
        className="flex shrink-0 select-none items-center gap-[5px] px-3 pb-1 pt-2.5 text-[10px] font-semibold tracking-wide text-fg-4 transition-colors hover:text-fg-3"
      >
        <Boxes size={10} />
        <span className="uppercase">workspaces</span>
        {open ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
        <span className="ml-auto flex items-center gap-0.5">
          <button
            type="button"
            title="新建工作区"
            aria-label="新建工作区"
            disabled={submitting}
            data-testid="sidebar-new-workspace"
            onClick={(e) => {
              e.stopPropagation();
              if (open) {
                setCreating((v) => !v);
                resetForm();
              } else {
                openPanel(true);
              }
            }}
            className="flex h-[22px] w-[22px] items-center justify-center rounded-[5px] text-fg-3 transition-colors hover:bg-shell-hover"
          >
            <Plus size={11} />
          </button>
        </span>
      </div>

      {open && (
        <div
          data-testid="workspace-picker-panel"
          onKeyDown={(e) => {
            if (e.key === 'Escape') closePanel();
          }}
          className="flex min-h-0 flex-1 flex-col border-t border-edge bg-shell-float"
        >
          <div className="flex h-[30px] shrink-0 items-center gap-1.5 border-b border-edge px-2.5">
            <Search size={12} className="shrink-0 text-fg-3" />
            <input
              autoFocus
              value={query}
              data-testid="workspace-picker-search"
              onChange={(e) => setQuery(e.target.value)}
              placeholder="搜索文件夹、工作区…"
              className="h-full min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-4"
            />
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto px-1 pb-1">
            <PickerSecHead label="最近" />
            {filtered.length === 0 && (
              <p className="px-2.5 py-1 text-[11px] text-fg-4">
                {ordered.length === 0 ? '还没有工作区,用下方入口添加。' : '没有匹配的工作区。'}
              </p>
            )}
            {filtered.map((w) => (
              <div
                key={w.id}
                role="button"
                tabIndex={0}
                title={displayRoot(w.root)}
                data-testid={`workspace-row-${w.id}`}
                onClick={() => selectWorkspace(w.id)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') selectWorkspace(w.id);
                }}
                className={cn(
                  'group/ws flex items-center gap-2 rounded-md px-2.5 py-1 transition-colors hover:bg-shell-hover',
                  activeWorkspaceId === w.id && 'bg-shell-active',
                )}
              >
                <Folder size={13} className="shrink-0 text-fg-3" />
                <span className="flex min-w-0 flex-1 flex-col leading-tight">
                  <span className="truncate text-[12px] text-fg">{w.name}</span>
                  <span className="truncate font-code text-[10px] text-fg-4">
                    {displayRoot(w.root)}
                  </span>
                </span>
                {activeWorkspaceId === w.id && <Check size={11} className="shrink-0 text-acc" />}
                <button
                  type="button"
                  title="删除工作区"
                  aria-label="删除工作区"
                  onClick={(e) => {
                    e.stopPropagation();
                    void removeWorkspace(w.id);
                  }}
                  className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-fg-3 opacity-0 transition-opacity hover:bg-danger-bg group-hover/ws:opacity-100"
                >
                  <Trash2 size={10} />
                </button>
              </div>
            ))}

            <div className="my-1 h-px" style={{ background: 'var(--line)' }} />

            <PickerSecHead label="打开" />
            <PickerRow
              icon={<Boxes size={13} />}
              label="全部会话"
              hint="不限工作区"
              selected={activeWorkspaceId === null}
              testId="workspace-row-all"
              onSelect={() => selectWorkspace(null)}
            />
            <PickerRow
              icon={<HardDrive size={13} />}
              label={picking ? '正在选择目录…' : '本机目录…'}
              chevron
              disabled={pickFolder === undefined || picking || submitting}
              title={
                pickFolder === undefined
                  ? '仅桌面端可用(系统目录对话框);浏览器下请用「新建工作区…」手填路径'
                  : '系统目录对话框选工作区根目录'
              }
              testId="workspace-pick-folder"
              onSelect={onPickFolder}
            />
            <PickerRow
              icon={<FolderPlus size={13} />}
              label="新建工作区…"
              chevron
              selected={creating}
              disabled={submitting}
              testId="workspace-new-inline"
              onSelect={() => {
                setCreating((v) => !v);
                resetForm();
              }}
            />

            {creating && (
              <div aria-busy={submitting} className="mx-0.5 mt-1 flex flex-col gap-1 rounded-md border border-acc-ring bg-shell-panel p-2">
                <div className="flex items-center gap-1.5">
                  <Boxes size={12} className="shrink-0 text-fg-3" />
                  <input
                    autoFocus
                    value={name}
                    disabled={submitting}
                    aria-label="工作区名称"
                    data-testid="sidebar-workspace-name"
                    onChange={(e) => setName(e.target.value)}
                    onKeyDown={onFormKeyDown}
                    placeholder="工作区名称…"
                    className="min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-4"
                  />
                </div>
                <input
                  value={root}
                  disabled={submitting}
                  aria-label="工作区根目录（选填）"
                  data-testid="sidebar-workspace-root"
                  onChange={(e) => setRoot(e.target.value)}
                  onKeyDown={onFormKeyDown}
                  placeholder="根目录绝对路径（选填）…"
                  className="w-full bg-transparent px-0.5 font-code text-[11px] text-fg outline-none placeholder:text-fg-4"
                />
                <span className="px-0.5 text-[10px] text-fg-4">留空自动创建项目目录；填写路径时，不存在的目录会自动创建。</span>
                {/* F-GAME-3:游戏选型——制作前定 2D/3D,写入 forge.toml,引擎/Agent 全链路透传 */}
                <div className="flex flex-col gap-1">
                  <span className="px-0.5 text-[10px] text-fg-4">游戏类型(写入项目 forge.toml)</span>
                  <div className="flex gap-1" role="radiogroup" aria-label="游戏类型">
                    {(
                      [
                        ['2d', '2D 游戏', 'Godot · XY 平面 · 正交相机 · Sprite 精灵'],
                        ['3d', '3D 游戏', '可选 Godot 或 rurix · 透视相机 · 3D 物理'],
                        ['none', '仅目录', '不初始化项目'],
                      ] as Array<[GameTypeChoice, string, string]>
                    ).map(([value, label, tip]) => (
                      <button
                        key={value}
                        type="button"
                        disabled={submitting}
                        role="radio"
                        aria-checked={gameType === value}
                        title={tip}
                        data-testid={`workspace-gametype-${value}`}
                        onClick={() => setGameType(value)}
                        className={cn(
                          'h-[22px] flex-1 rounded-[5px] border text-[11px] transition-colors',
                          gameType === value
                            ? 'border-acc-ring bg-shell-active text-acc'
                            : 'border-edge text-fg-3 hover:bg-shell-hover',
                        )}
                      >
                        {label}
                      </button>
                    ))}
                  </div>
                  {gameType === '2d' && (
                    <span data-testid="workspace-backend-2d" className="px-0.5 text-[10px] text-fg-3">
                      实现后端：Godot（默认）
                    </span>
                  )}
                  {gameType === '3d' && (
                    <div className="flex flex-col gap-1">
                      <span className="px-0.5 text-[10px] text-fg-4">实现后端</span>
                      <div className="flex gap-1" role="radiogroup" aria-label="3D 实现后端">
                        {(['godot', 'rurix'] as GameBackendChoice[]).map((value) => (
                          <button
                            key={value}
                            type="button"
                            disabled={submitting}
                            role="radio"
                            aria-checked={backend3d === value}
                            data-testid={`workspace-backend-${value}`}
                            onClick={() => setBackend3d(value)}
                            className={cn(
                              'h-[22px] flex-1 rounded-[5px] border text-[11px] transition-colors',
                              backend3d === value
                                ? 'border-acc-ring bg-shell-active text-acc'
                                : 'border-edge text-fg-3 hover:bg-shell-hover',
                            )}
                          >
                            {value === 'godot' ? 'Godot' : 'rurix'}
                          </button>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
                {createError !== null && <p role="alert" className="px-0.5 text-[11px] text-danger">{createError}</p>}
                <button
                  type="button"
                  disabled={submitting || name.trim() === ''}
                  title={name.trim() === '' ? '请填写工作区名称' : undefined}
                  onClick={() => void submitWorkspace()}
                  className="self-end text-[11px] text-acc disabled:opacity-40"
                  data-testid="sidebar-workspace-create"
                >
                  {submitting ? '创建中…' : '创建'}
                </button>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
