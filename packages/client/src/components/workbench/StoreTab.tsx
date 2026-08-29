import { useEffect, useRef, useState } from 'react';
import {
  AlertTriangle,
  ArrowUp,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Download,
  ExternalLink,
  KeyRound,
  Package,
  Plus,
  RefreshCw,
  Search,
  Server,
  Sparkles,
  Trash2,
  X,
} from 'lucide-react';
import { cn } from '@/lib/cn';
import { useAssetStore } from '@/lib/assetStore';
import type {
  InstallRecord,
  LibraryItem,
  PackagePricing,
  PackageSummary,
  SearchHit,
} from '@/lib/storeApi';
import {
  phaseLabel,
  pkgKey,
  storeErrorText,
  useStoreStore,
  type KindFilter,
  type StoreSubTab,
} from '@/lib/storeStore';

/**
 * F11 wave.4 资产商店 tab(契约 11_API_CONTRACTS.md §2.7,后端 wave.3 并行落地)。
 *
 * 三子页(发现 / 已安装 / 我的资产库)+ 右侧 360px 详情分栏 + 同 tab 内展开的源管理面板。
 * 全程非模态:确认、提案批准、源管理都走行内展开条,绝不用 <dialog>/confirm/alert
 * ——模态会永久阻塞无人值守冒烟(见 F1 坑,AssetsPanel 同注)。
 *
 * 诚实纪律三处落点:
 *  1. search 的 errors[](不可达的源)顶起黄色横幅,可展开看每源 code + message,不静默吞;
 *  2. 安装 failed 时可读文案与错误码并列显示,不化简成「失败了」;
 *  3. 加载/错误/空三态分开呈现,空列表不用来掩盖请求失败。
 *
 * R-5 密钥红线:源 token 只单向送出(新增/改源表单走 uncontrolled password 输入,提交即清),
 * 任何位置都不回显已存 token,列表只显示「已配置」徽标(源 DTO 本身也只有 hasToken 布尔)。
 */

const SUB_TABS: Array<{ id: StoreSubTab; label: string }> = [
  { id: 'discover', label: '发现' },
  { id: 'installed', label: '已安装' },
  { id: 'library', label: '我的资产库' },
];

const KIND_FILTERS: Array<{ id: KindFilter; label: string }> = [
  { id: 'all', label: '全部' },
  { id: 'asset-pack', label: '资产包' },
  { id: 'skill', label: '技能' },
];

const KIND_LABEL: Record<string, string> = { 'asset-pack': '资产包', skill: '技能' };

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) {
    const kb = n / 1024;
    return `${kb < 10 ? kb.toFixed(1) : Math.round(kb)} KB`;
  }
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function formatTime(iso: string): string {
  if (!iso) return '—';
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString('zh-CN', { hour12: false });
}

function priceAmountText(p: PackagePricing): string {
  const n = p.amount;
  const num = Number.isInteger(n) ? String(n) : n.toFixed(2);
  return p.currency === 'CNY' ? `¥${num}` : `${num} ${p.currency}`;
}

/** 付费包当前一律不可获取(无支付通道),按钮禁用并如实标注。 */
function isPaid(pricing?: PackagePricing | null): boolean {
  return !!pricing && pricing.amount > 0;
}

const iconBtn =
  'flex h-[22px] w-[22px] items-center justify-center rounded text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2';
const chipCls = 'flex h-[17px] shrink-0 items-center rounded-full border border-edge bg-shell-sunk px-1.5 text-[10.5px] text-fg-3';
const primaryBtn =
  'flex h-[22px] shrink-0 items-center gap-1 rounded-md border border-acc bg-acc px-2 text-[11px] text-fg-inv transition-colors hover:bg-acc-soft disabled:cursor-not-allowed disabled:opacity-40';
const ghostBtn =
  'flex h-[22px] shrink-0 items-center gap-1 rounded-md border border-edge bg-shell-panel px-2 text-[11px] text-fg-2 transition-colors hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-40';
const inputCls =
  'h-[24px] min-w-0 rounded-md border border-edge bg-shell-input px-2 text-[12px] text-fg outline-none placeholder:text-fg-4 focus:border-acc-ring';

function KindBadge({ kind }: { kind: string }) {
  const Icon = kind === 'skill' ? Sparkles : Package;
  return (
    <span className={chipCls}>
      <Icon size={9} className="mr-1" />
      {KIND_LABEL[kind] ?? kind}
    </span>
  );
}

/** 缩略图区:无 thumbnail 时以类型图标占位,不拉外链假图。 */
function Thumbnail({ pkg, compact }: { pkg: { kind: string; thumbnail?: string | null; name: string }; compact?: boolean }) {
  const Icon = pkg.kind === 'skill' ? Sparkles : Package;
  return (
    <div
      className={cn(
        'flex w-full items-center justify-center overflow-hidden rounded-t-lg border-b border-edge bg-shell-sunk',
        compact ? 'h-[52px]' : 'h-[84px]',
      )}
    >
      {pkg.thumbnail ? (
        <img src={pkg.thumbnail} alt={pkg.name} className="h-full w-full object-cover" />
      ) : (
        <Icon size={compact ? 16 : 22} className="text-fg-4" strokeWidth={1.4} />
      )}
    </div>
  );
}

// ---------- 长任务进度 / 错误 ----------

function TaskStrip() {
  const task = useStoreStore((s) => s.task);
  const taskKind = useStoreStore((s) => s.taskKind);
  const taskError = useStoreStore((s) => s.taskError);
  if (!task && !taskError) return null;
  const pct = task && task.total > 0 ? Math.min(100, Math.round((task.done / task.total) * 100)) : 0;
  return (
    <div className="flex flex-col gap-1 rounded-md border border-edge bg-shell-sunk px-2.5 py-1.5" data-testid="store-task-progress">
      {task && (
        <>
          <div className="flex items-center gap-2">
            <span className="min-w-0 flex-1 truncate text-[12px] text-fg-2" data-testid="store-task-phase">
              {taskKind === 'uninstall' ? '卸载' : '安装'} · {phaseLabel(task)}
            </span>
            <span className="shrink-0 font-code text-[10.5px] text-fg-4">{task.status}</span>
          </div>
          <div className="h-1 w-full overflow-hidden rounded-full bg-shell-active">
            <div
              className={cn('h-full rounded-full', task.status === 'failed' ? 'bg-danger' : 'bg-acc')}
              style={{ width: `${task.status === 'completed' ? 100 : pct}%` }}
              data-testid="store-task-bar"
            />
          </div>
        </>
      )}
      {taskError && (
        <div className="flex flex-col gap-0.5" data-testid="store-task-error">
          <div className="flex items-center gap-1.5">
            <AlertTriangle size={11} className="shrink-0 text-danger" />
            <span className="min-w-0 flex-1 text-[12px] text-danger">
              {storeErrorText(taskError.code, taskError.message)}
            </span>
            <span className="shrink-0 rounded-full bg-danger-bg px-1.5 font-code text-[10.5px] text-danger" data-testid="store-task-error-code">
              {taskError.code}
            </span>
          </div>
          <span className="pl-[17px] text-[10.5px] text-fg-4">{taskError.message}</span>
        </div>
      )}
    </div>
  );
}

// ---------- 发现页 ----------

function SourceErrorBanner({ errors }: { errors: Array<{ sourceId: string; code: string; message: string }> }) {
  const [open, setOpen] = useState(false);
  if (errors.length === 0) return null;
  return (
    <div className="flex flex-col rounded-md border border-warn bg-warn-bg px-2.5 py-1.5" data-testid="store-source-errors">
      <button
        type="button"
        aria-label="展开不可达源明细"
        data-testid="store-source-errors-toggle"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1.5 text-left"
      >
        <AlertTriangle size={12} className="shrink-0 text-warn" />
        <span className="min-w-0 flex-1 text-[12px] text-warn">{errors.length} 个源不可达</span>
        {open ? <ChevronDown size={11} className="text-warn" /> : <ChevronRight size={11} className="text-warn" />}
      </button>
      {open && (
        <div className="mt-1 flex flex-col gap-0.5 border-t border-warn pt-1">
          {errors.map((e) => (
            <div key={e.sourceId} className="flex items-center gap-1.5" data-testid={`store-source-error-${e.sourceId}`}>
              <span className="shrink-0 font-code text-[10.5px] text-fg-2">{e.sourceId}</span>
              <span className="shrink-0 rounded-full bg-shell-panel px-1.5 font-code text-[10.5px] text-warn">{e.code}</span>
              <span className="min-w-0 flex-1 truncate text-[11px] text-fg-3" title={e.message}>
                {e.message}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function PackageCard({
  hit,
  record,
  onOpen,
  onInstall,
}: {
  hit: SearchHit;
  record: InstallRecord | undefined;
  onOpen: () => void;
  onInstall: (version: string) => void;
}) {
  const pkg = hit.package;
  const paid = isPaid(pkg.pricing);
  const canUpdate = record !== undefined && record.version !== pkg.latestVersion;
  return (
    <div
      role="button"
      tabIndex={0}
      aria-label={`包 ${pkg.name}`}
      data-testid={`store-card-${pkg.id}`}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onOpen();
        }
      }}
      className="flex cursor-pointer flex-col rounded-lg border border-edge bg-shell-panel text-left shadow-sh1 transition-colors hover:border-edge-strong hover:bg-shell-hover"
    >
      <Thumbnail pkg={pkg} />
      <div className="flex min-w-0 flex-1 flex-col gap-1 p-2">
        <div className="flex items-center gap-1.5">
          <span className="min-w-0 flex-1 truncate text-[12.4px] font-medium text-fg" title={pkg.name}>
            {pkg.name}
          </span>
          <span className="shrink-0 font-code text-[10.5px] text-fg-4">v{pkg.latestVersion}</span>
        </div>
        <div className="flex items-center gap-1">
          <KindBadge kind={pkg.kind} />
          <span className="min-w-0 truncate text-[10.5px] text-fg-4" title={hit.sourceName}>
            {pkg.publisher?.name ?? '未署名发布者'} · {hit.sourceName}
          </span>
        </div>
        <p className="line-clamp-2 text-[11px] leading-[1.45] text-fg-3">{pkg.description}</p>
        <div className="flex flex-wrap items-center gap-1">
          {pkg.tags.slice(0, 3).map((t) => (
            <span key={t} className="rounded-full bg-shell-sunk px-1.5 text-[10px] text-fg-4">
              {t}
            </span>
          ))}
        </div>
        <div className="mt-auto flex items-center gap-1.5 pt-1">
          {pkg.license ? <span className={chipCls}>{pkg.license.id}</span> : <span className={chipCls}>许可证未标注</span>}
          <span
            className={cn('shrink-0 text-[10.5px]', paid ? 'text-fg-2' : 'text-fg-4')}
            data-testid={`store-card-price-${pkg.id}`}
          >
            {pkg.pricing ? (pkg.pricing.amount === 0 ? '免费' : priceAmountText(pkg.pricing)) : '未标价'}
          </span>
          <span className="min-w-0 flex-1" />
          {paid ? (
            <span className="flex shrink-0 items-center gap-1">
              <span className="text-[10px] text-fg-4">暂不支持付费获取</span>
              <button type="button" disabled className={primaryBtn} data-testid={`store-card-install-${pkg.id}`}>
                安装
              </button>
            </span>
          ) : canUpdate ? (
            <button
              type="button"
              className={primaryBtn}
              data-testid={`store-card-install-${pkg.id}`}
              onClick={(e) => {
                e.stopPropagation();
                onInstall(pkg.latestVersion);
              }}
            >
              <ArrowUp size={10} />
              更新到 {pkg.latestVersion}
            </button>
          ) : record ? (
            <button type="button" disabled className={ghostBtn} data-testid={`store-card-install-${pkg.id}`}>
              已安装
            </button>
          ) : (
            <button
              type="button"
              className={primaryBtn}
              data-testid={`store-card-install-${pkg.id}`}
              onClick={(e) => {
                e.stopPropagation();
                onInstall(pkg.latestVersion);
              }}
            >
              <Download size={10} />
              安装
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function DiscoverPage() {
  const st = useStoreStore();
  const { search, searchLoading, searchError } = st;
  const totalPages = search && search.pageSize > 0 ? Math.max(1, Math.ceil(search.total / search.pageSize)) : 1;
  const recordOf = (h: SearchHit) =>
    st.installed.find((r) => r.sourceId === h.sourceId && r.packageId === h.package.id);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-3" data-testid="store-discover">
      {/* 搜索行:回车触发 + 类型 chips + 源过滤 */}
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex h-[26px] w-[220px] min-w-[160px] items-center gap-1.5 rounded-md border border-edge bg-shell-input px-2">
          <Search size={12} className="shrink-0 text-fg-4" />
          <input
            value={st.query}
            onChange={(e) => st.setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                st.setPage(1);
              }
            }}
            aria-label="搜索包"
            placeholder="搜索包名 / 标签…回车检索"
            data-testid="store-search-input"
            className="min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-4"
          />
        </div>
        <span className="flex items-center gap-1 rounded-md bg-shell-sunk p-0.5">
          {KIND_FILTERS.map((k) => (
            <button
              key={k.id}
              type="button"
              data-testid={`store-kind-${k.id}`}
              onClick={() => st.setKindFilter(k.id)}
              className={cn(
                'rounded px-2 py-0.5 text-[11px] transition-colors',
                st.kindFilter === k.id ? 'bg-shell-panel text-fg shadow-sm' : 'text-fg-3 hover:text-fg-2',
              )}
            >
              {k.label}
            </button>
          ))}
        </span>
        <select
          value={st.sourceFilter}
          onChange={(e) => st.setSourceFilter(e.target.value)}
          aria-label="按源过滤"
          data-testid="store-source-filter"
          className={cn(inputCls, 'w-[150px]')}
        >
          <option value="">全部源</option>
          {st.sources.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
      </div>

      {search && <SourceErrorBanner errors={search.errors ?? []} />}

      {searchLoading && (
        <span className="text-[12px] text-fg-4" data-testid="store-search-loading">
          加载中…
        </span>
      )}

      {!searchLoading && searchError && (
        <div className="flex items-start gap-1.5 rounded-md border border-danger bg-danger-bg px-2.5 py-1.5" data-testid="store-search-error">
          <AlertTriangle size={12} className="mt-0.5 shrink-0 text-danger" />
          <span className="min-w-0 flex-1 text-[12px] text-danger">检索失败:{searchError}</span>
        </div>
      )}

      {!searchLoading && !searchError && search && search.items.length === 0 && (
        <div className="flex flex-col gap-1 py-6 text-center" data-testid="store-empty">
          <span className="text-[12.4px] text-fg-2">
            未找到匹配的包{st.submittedQuery.trim() === '' ? '' : `（检索词「${st.submittedQuery.trim()}」）`}
          </span>
          <span className="text-[11px] text-fg-4">
            换个关键词、把类型切回「全部」，或在「源管理」里确认源已启用。
          </span>
        </div>
      )}

      {search && search.items.length > 0 && (
        <div
          className="grid gap-2 [grid-template-columns:repeat(auto-fill,minmax(200px,1fr))]"
          data-testid="store-grid"
        >
          {search.items.map((h) => (
            <PackageCard
              key={`${h.sourceId}/${h.package.id}`}
              hit={h}
              record={recordOf(h)}
              onOpen={() => void st.openDetail(h.sourceId, h.package.id)}
              // 卡上快装 = 按类型自动分流;要指定落地文件夹请开详情栏填 destFolder,
              // 不让详情栏里看不见的输入值悄悄作用到网格里的另一个包。
              onInstall={(v) => void st.install(h.sourceId, h.package.id, v)}
            />
          ))}
        </div>
      )}

      {search && search.total > 0 && (
        <div className="flex items-center justify-center gap-2 pt-1" data-testid="store-pager">
          <button
            type="button"
            className={ghostBtn}
            disabled={st.page <= 1}
            data-testid="store-page-prev"
            onClick={() => st.setPage(st.page - 1)}
          >
            <ChevronLeft size={10} />
            上一页
          </button>
          <span className="text-[11px] text-fg-3" data-testid="store-page-indicator">
            第 {search.page} / {totalPages} 页 · 共 {search.total} 个包
          </span>
          <button
            type="button"
            className={ghostBtn}
            disabled={st.page >= totalPages}
            data-testid="store-page-next"
            onClick={() => st.setPage(st.page + 1)}
          >
            下一页
            <ChevronRight size={10} />
          </button>
        </div>
      )}
    </div>
  );
}

// ---------- 详情分栏 ----------

function DetailPanel() {
  const st = useStoreStore();
  const { detail, manifest } = st;
  if (!detail) return null;
  const pkg: PackageSummary = detail.summary;
  const paid = isPaid(pkg.pricing);
  const version = st.detailVersion ?? pkg.latestVersion;

  return (
    <aside
      className="flex w-[360px] shrink-0 flex-col overflow-y-auto border-l border-edge bg-shell-panel"
      aria-label="包详情"
      data-testid="store-detail"
    >
      <div className="flex items-start gap-2 border-b border-edge px-3 py-2">
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-medium text-fg" title={pkg.name}>
            {pkg.name}
          </p>
          <p className="truncate font-code text-[10.5px] text-fg-4">{pkg.id}</p>
        </div>
        <button
          type="button"
          aria-label="关闭详情"
          data-testid="store-detail-close"
          className={iconBtn}
          onClick={st.closeDetail}
        >
          <X size={12} />
        </button>
      </div>

      <div className="flex flex-col gap-2 px-3 py-2">
        <TaskStrip />

        {st.detailLoading && <span className="text-[12px] text-fg-4">加载中…</span>}
        {st.detailError && (
          <span className="text-[12px] text-danger" data-testid="store-detail-error">
            {st.detailError}
          </span>
        )}

        {/* 版本 + 发布者 + 许可证 */}
        <div className="flex flex-wrap items-center gap-2">
          <label className="flex items-center gap-1 text-[11px] text-fg-4">
            版本
            <select
              value={version}
              onChange={(e) => void st.selectVersion(e.target.value)}
              aria-label="选择版本"
              data-testid="store-detail-version"
              className={cn(inputCls, 'w-[110px]')}
            >
              {(detail.versions.length > 0 ? detail.versions : [pkg.latestVersion]).map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </label>
          <KindBadge kind={pkg.kind} />
        </div>

        <div className="flex flex-col gap-0.5 text-[11px] text-fg-3">
          <span>
            发布者：{pkg.publisher?.name ?? '未署名'}
            {pkg.publisher?.url && (
              <a
                href={pkg.publisher.url}
                target="_blank"
                rel="noreferrer"
                className="ml-1 inline-flex items-center gap-0.5 text-acc"
              >
                主页
                <ExternalLink size={9} />
              </a>
            )}
          </span>
          <span>
            许可证：
            {pkg.license ? (
              pkg.license.url ? (
                <a href={pkg.license.url} target="_blank" rel="noreferrer" className="inline-flex items-center gap-0.5 text-acc" data-testid="store-detail-license">
                  {pkg.license.id}
                  <ExternalLink size={9} />
                </a>
              ) : (
                <span data-testid="store-detail-license">{pkg.license.id}</span>
              )
            ) : (
              <span data-testid="store-detail-license">未标注</span>
            )}
          </span>
          <span data-testid="store-detail-price">
            价格：{pkg.pricing ? (pkg.pricing.amount === 0 ? '免费' : priceAmountText(pkg.pricing)) : '未标价'}
            {paid && <span className="ml-1 text-fg-4">暂不支持付费获取</span>}
          </span>
          {manifest?.engineVersion && <span>引擎版本要求：{manifest.engineVersion}</span>}
        </div>

        <p className="whitespace-pre-wrap text-[12px] leading-[1.5] text-fg-2">{pkg.description}</p>

        {pkg.tags.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {pkg.tags.map((t) => (
              <span key={t} className="rounded-full bg-shell-sunk px-1.5 text-[10px] text-fg-4">
                {t}
              </span>
            ))}
          </div>
        )}
      </div>

      {/* 文件清单 */}
      <div className="border-t border-edge px-3 py-2">
        <p className="pb-1 text-[11px] font-medium text-fg-2">
          文件清单{manifest ? `（${manifest.files.length}）` : ''}
        </p>
        {manifest ? (
          <table className="w-full table-fixed border-collapse" data-testid="store-detail-files">
            <thead>
              <tr className="text-left text-[10px] text-fg-4">
                <th className="w-[52%] font-normal">路径</th>
                <th className="w-[22%] font-normal">大小</th>
                <th className="w-[26%] font-normal">sha256</th>
              </tr>
            </thead>
            <tbody>
              {manifest.files.map((f) => (
                <tr key={f.path} data-testid={`store-detail-file-${f.path}`} className="align-top">
                  <td className="truncate pr-1 text-[11px] text-fg-2" title={f.path}>
                    {f.path}
                  </td>
                  <td className="text-[11px] text-fg-3">{formatBytes(f.size)}</td>
                  <td className="font-code text-[10.5px] text-fg-4" title={f.sha256}>
                    {f.sha256.slice(0, 8)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <span className="text-[11px] text-fg-4">{st.detailError ? '清单不可用' : '清单加载中…'}</span>
        )}
      </div>

      {/* 依赖 */}
      <div className="border-t border-edge px-3 py-2" data-testid="store-detail-deps">
        <p className="pb-1 text-[11px] font-medium text-fg-2">依赖</p>
        {manifest && manifest.dependencies.length > 0 ? (
          <div className="flex flex-col gap-0.5">
            {manifest.dependencies.map((d) => (
              <span key={d.id} className="font-code text-[10.5px] text-fg-3">
                {d.id} @ {d.version}
              </span>
            ))}
          </div>
        ) : (
          <span className="text-[11px] text-fg-4">无依赖</span>
        )}
      </div>

      {/* 安装动作 */}
      <div className="mt-auto flex flex-col gap-1.5 border-t border-edge px-3 py-2">
        <input
          value={st.destFolder}
          onChange={(e) => st.setDestFolder(e.target.value)}
          aria-label="安装目标文件夹"
          placeholder="安装目标文件夹（留空则按类型自动分流）"
          data-testid="store-detail-dest"
          className={cn(inputCls, 'w-full')}
        />
        <div className="flex items-center gap-2">
          <button
            type="button"
            className={primaryBtn}
            disabled={paid || st.installing || st.pollingTaskId !== null}
            data-testid="store-detail-install"
            onClick={() => void st.install(detail.sourceId, pkg.id, version, st.destFolder)}
          >
            <Download size={10} />
            {st.installing ? '提交中…' : '安装'}
          </button>
          <button type="button" className={ghostBtn} data-testid="store-detail-cancel" onClick={st.closeDetail}>
            取消
          </button>
          {paid && <span className="text-[10.5px] text-fg-4">暂不支持付费获取</span>}
        </div>
      </div>
    </aside>
  );
}

// ---------- 已安装页 ----------

function UninstallBar() {
  const pu = useStoreStore((s) => s.pendingUninstall);
  const confirmUninstall = useStoreStore((s) => s.confirmUninstall);
  const approveUninstall = useStoreStore((s) => s.approveUninstall);
  const cancelUninstall = useStoreStore((s) => s.cancelUninstall);
  if (!pu) return null;
  return (
    <div
      className="mt-1 flex flex-col gap-1 rounded-md border border-edge-strong bg-shell-sunk px-2 py-1.5"
      data-testid="store-uninstall-bar"
    >
      <div className="flex items-center gap-2">
        {pu.stage === 'confirm' && (
          <>
            <span className="min-w-0 flex-1 text-[11px] text-fg-2">
              卸载 {pu.packageId}？已落地的资产与技能将被移除。
            </span>
            <button type="button" className={primaryBtn} data-testid="store-uninstall-confirm" onClick={() => void confirmUninstall()}>
              确认卸载
            </button>
          </>
        )}
        {pu.stage === 'proposal' && (
          <>
            <span className="min-w-0 flex-1 text-[11px] text-warn" data-testid="store-uninstall-proposal">
              卸载属破坏性操作，已创建提案 {pu.proposalId}，批准后自动重发。
            </span>
            <button type="button" className={primaryBtn} data-testid="store-uninstall-approve" onClick={() => void approveUninstall()}>
              在此批准
            </button>
          </>
        )}
        {pu.stage === 'working' && (
          <span className="min-w-0 flex-1 text-[11px] text-fg-3">提案已批准，正在卸载…</span>
        )}
        <button type="button" className={ghostBtn} data-testid="store-uninstall-cancel" onClick={cancelUninstall}>
          取消
        </button>
      </div>
      {pu.error && (
        <span className="text-[11px] text-danger" data-testid="store-uninstall-error">
          {pu.error}
        </span>
      )}
    </div>
  );
}

function InstalledPage() {
  const st = useStoreStore();
  const pu = st.pendingUninstall;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-3" data-testid="store-installed">
      <div className="flex items-center gap-2">
        <span className="text-[12px] text-fg-3">已安装 {st.installed.length} 个包</span>
        <span className="flex-1" />
        <button
          type="button"
          className={ghostBtn}
          disabled={st.updatesLoading}
          data-testid="store-check-updates"
          onClick={() => void st.checkUpdates()}
        >
          <RefreshCw size={10} />
          {st.updatesLoading ? '检查中…' : '检查更新'}
        </button>
      </div>

      <TaskStrip />

      {st.updatesError && (
        <span className="text-[11px] text-danger" data-testid="store-updates-error">
          更新检查失败：{st.updatesError}
        </span>
      )}
      {st.installedLoading && <span className="text-[12px] text-fg-4">加载中…</span>}
      {!st.installedLoading && st.installedError && (
        <div className="flex items-start gap-1.5 rounded-md border border-danger bg-danger-bg px-2.5 py-1.5" data-testid="store-installed-error">
          <AlertTriangle size={12} className="mt-0.5 shrink-0 text-danger" />
          <span className="min-w-0 flex-1 text-[12px] text-danger">已安装清单加载失败：{st.installedError}</span>
        </div>
      )}
      {!st.installedLoading && !st.installedError && st.installed.length === 0 && (
        <span className="py-6 text-center text-[12px] text-fg-4" data-testid="store-installed-empty">
          尚未安装任何包。到「发现」页挑一个装上。
        </span>
      )}

      <div className="flex flex-col gap-1.5" data-testid="store-installed-list">
        {st.installed.map((r) => {
          const key = pkgKey(r.sourceId, r.packageId);
          const up = st.updates.find((u) => u.sourceId === r.sourceId && u.packageId === r.packageId && u.hasUpdate);
          const active = pu?.sourceId === r.sourceId && pu?.packageId === r.packageId;
          return (
            <div
              key={key}
              className="flex flex-col rounded-lg border border-edge bg-shell-panel px-2.5 py-1.5 shadow-sh1"
              data-testid={`store-installed-row-${key}`}
            >
              <div className="flex items-center gap-2">
                <KindBadge kind={r.kind} />
                <span className="min-w-0 flex-1 truncate text-[12.4px] text-fg" title={r.packageId}>
                  {r.packageId}
                </span>
                <span className="shrink-0 font-code text-[10.5px] text-fg-3">v{r.version}</span>
                <span className="shrink-0 text-[10.5px] text-fg-4">来源 {r.sourceId}</span>
                <span className="shrink-0 text-[10.5px] text-fg-4">{formatTime(r.installedAt)}</span>
                <span className="shrink-0 text-[10.5px] text-fg-4">
                  资产 {r.assetPaths.length} · 技能 {r.skillNames.length}
                </span>
                {up && (
                  <button
                    type="button"
                    className={primaryBtn}
                    data-testid={`store-update-${key}`}
                    onClick={() => void st.install(r.sourceId, r.packageId, up.latest)}
                  >
                    <ArrowUp size={10} />
                    更新到 {up.latest}
                  </button>
                )}
                <button
                  type="button"
                  className={ghostBtn}
                  data-testid={`store-uninstall-${key}`}
                  onClick={() => st.requestUninstall(r.sourceId, r.packageId)}
                >
                  <Trash2 size={10} />
                  卸载
                </button>
              </div>
              {active && <UninstallBar />}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ---------- 我的资产库页 ----------

function LibraryCard({ item, onInstall, onRemove }: { item: LibraryItem; onInstall: () => void; onRemove: () => void }) {
  return (
    <div
      className="flex flex-col rounded-lg border border-edge bg-shell-panel shadow-sh1 transition-colors hover:border-edge-strong hover:bg-shell-hover"
      data-testid={`store-library-item-${item.id}`}
    >
      <Thumbnail pkg={{ kind: item.kind, name: item.name, thumbnail: null }} compact />
      <div className="flex flex-col gap-1 p-2">
        <span className="truncate text-[12.4px] text-fg" title={item.name}>
          {item.name}
        </span>
        <div className="flex flex-wrap items-center gap-1 text-[10.5px] text-fg-4">
          <span className={chipCls}>{item.kind}</span>
          <span>{formatBytes(item.size)}</span>
          <span className={chipCls}>{item.source}</span>
        </div>
        <span className="text-[10px] text-fg-4">{formatTime(item.addedAt)}</span>
        <div className="flex items-center gap-1.5 pt-0.5">
          <button type="button" className={primaryBtn} data-testid={`store-library-install-${item.id}`} onClick={onInstall}>
            <Download size={10} />
            装进项目
          </button>
          <button type="button" className={ghostBtn} data-testid={`store-library-remove-${item.id}`} onClick={onRemove}>
            <Trash2 size={10} />
            移出库
          </button>
        </div>
      </div>
    </div>
  );
}

function LibraryPage() {
  const st = useStoreStore();
  const [path, setPath] = useState('');
  const [dest, setDest] = useState('');
  // assetStore 只读引用:取 Assets 面板当前选中项做「收藏选中资产」快捷键,不写回它。
  const selectedAsset = useAssetStore((s) => s.items.find((i) => i.guid === s.selectedGuid));

  const add = (p: string) => {
    const v = p.trim();
    if (v === '') return;
    void st.addToLibrary(v);
    setPath('');
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-3" data-testid="store-library">
      <div className="flex flex-wrap items-center gap-2">
        <input
          value={path}
          onChange={(e) => setPath(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') add(path);
          }}
          aria-label="从当前项目收藏"
          placeholder="资产相对路径，如 Meshes/chair.gltf"
          data-testid="store-library-path-input"
          className={cn(inputCls, 'w-[260px]')}
        />
        <button type="button" className={primaryBtn} data-testid="store-library-add" onClick={() => add(path)}>
          <Plus size={10} />
          收藏进库
        </button>
        <input
          value={dest}
          onChange={(e) => setDest(e.target.value)}
          aria-label="装进项目的目标文件夹"
          placeholder="目标文件夹（留空则按类型自动分流）"
          data-testid="store-library-dest"
          className={cn(inputCls, 'w-[220px]')}
        />
        <button
          type="button"
          className={ghostBtn}
          disabled={!selectedAsset}
          title={selectedAsset ? selectedAsset.path : '先在资产面板选中一件素材'}
          data-testid="store-library-from-selection"
          onClick={() => selectedAsset && add(selectedAsset.path)}
        >
          收藏选中资产
        </button>
      </div>

      {st.libraryLoading && <span className="text-[12px] text-fg-4">加载中…</span>}
      {!st.libraryLoading && st.libraryError && (
        <div className="flex items-start gap-1.5 rounded-md border border-danger bg-danger-bg px-2.5 py-1.5" data-testid="store-library-error">
          <AlertTriangle size={12} className="mt-0.5 shrink-0 text-danger" />
          <span className="min-w-0 flex-1 text-[12px] text-danger">资产库加载失败：{st.libraryError}</span>
        </div>
      )}
      {!st.libraryLoading && !st.libraryError && st.library.length === 0 && (
        <span className="py-6 text-center text-[12px] text-fg-4" data-testid="store-library-empty">
          资产库还是空的。收藏项目里的素材，跨项目复用。
        </span>
      )}

      {st.library.length > 0 && (
        <div className="grid gap-2 [grid-template-columns:repeat(auto-fill,minmax(160px,1fr))]" data-testid="store-library-grid">
          {st.library.map((i) => (
            <LibraryCard
              key={i.id}
              item={i}
              onInstall={() => void st.installLibraryItem(i.id, dest.trim() === '' ? undefined : dest.trim())}
              onRemove={() => void st.removeFromLibrary(i.id)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

// ---------- 源管理面板(同 tab 内展开,非弹窗) ----------

function SourcesPanel() {
  const st = useStoreStore();
  const [id, setId] = useState('');
  const [name, setName] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  // token 走 uncontrolled ref:不进 React state、不进 store,提交即清——杜绝任何回显路径(R-5)。
  const tokenRef = useRef<HTMLInputElement>(null);

  const submit = () => {
    if (id.trim() === '' || baseUrl.trim() === '') return;
    const token = tokenRef.current?.value ?? '';
    void st
      .addSource({
        id: id.trim(),
        name: name.trim() === '' ? id.trim() : name.trim(),
        baseUrl: baseUrl.trim(),
        ...(token === '' ? {} : { token }),
      })
      .then(() => {
        setId('');
        setName('');
        setBaseUrl('');
        if (tokenRef.current) tokenRef.current.value = '';
      })
      .catch(() => {
        // 失败已由 store 出 toast;表单保留待改(token 依然清掉,不长期留在 DOM 里)
        if (tokenRef.current) tokenRef.current.value = '';
      });
  };

  return (
    <div className="flex shrink-0 flex-col gap-2 border-b border-edge bg-shell-sunk px-3 py-2" data-testid="store-sources-panel">
      <div className="flex items-center gap-2">
        <Server size={12} className="text-fg-3" />
        <span className="text-[12px] font-medium text-fg-2">源管理</span>
        <span className="flex-1" />
        <button type="button" aria-label="收起源管理" className={iconBtn} onClick={st.toggleSourcesPanel}>
          <X size={12} />
        </button>
      </div>

      {st.sourcesError && (
        <span className="text-[11px] text-danger" data-testid="store-sources-error">
          源清单加载失败：{st.sourcesError}
        </span>
      )}
      {!st.sourcesError && st.sources.length === 0 && !st.sourcesLoading && (
        <span className="text-[11px] text-fg-4" data-testid="store-sources-empty">
          还没有配置源。
        </span>
      )}

      <div className="flex flex-col gap-1">
        {st.sources.map((s) => (
          <div
            key={s.id}
            className="flex items-center gap-2 rounded-md border border-edge bg-shell-panel px-2 py-1"
            data-testid={`store-source-row-${s.id}`}
          >
            <span className="w-[110px] shrink-0 truncate text-[12px] text-fg" title={s.name}>
              {s.name}
            </span>
            <span className="min-w-0 flex-1 truncate font-code text-[10.5px] text-fg-4" title={s.baseUrl}>
              {s.baseUrl}
            </span>
            {s.hasToken ? (
              <span className={cn(chipCls, 'gap-1')} data-testid={`store-source-token-${s.id}`}>
                <KeyRound size={9} />
                已配置
              </span>
            ) : (
              <span className={chipCls} data-testid={`store-source-token-${s.id}`}>
                无 token
              </span>
            )}
            <button
              type="button"
              role="switch"
              aria-checked={s.enabled}
              aria-label={`启停源 ${s.name}`}
              data-testid={`store-source-toggle-${s.id}`}
              onClick={() => void st.setSourceEnabled(s.id, !s.enabled)}
              className={cn(
                'flex h-[16px] w-[28px] shrink-0 items-center rounded-full px-0.5 transition-colors',
                s.enabled ? 'bg-acc' : 'bg-shell-active',
              )}
            >
              <span
                className={cn(
                  'h-[12px] w-[12px] rounded-full bg-shell-panel transition-transform',
                  s.enabled && 'translate-x-[12px]',
                )}
              />
            </button>
            <button
              type="button"
              aria-label={`删除源 ${s.name}`}
              data-testid={`store-source-delete-${s.id}`}
              className={iconBtn}
              onClick={() => void st.removeSource(s.id)}
            >
              <Trash2 size={11} />
            </button>
          </div>
        ))}
      </div>

      {/* 新增源 */}
      <div className="flex flex-wrap items-center gap-1.5 border-t border-edge pt-2">
        <input value={id} onChange={(e) => setId(e.target.value)} aria-label="源 id" placeholder="id" data-testid="store-source-new-id" className={cn(inputCls, 'w-[110px]')} />
        <input value={name} onChange={(e) => setName(e.target.value)} aria-label="源名称" placeholder="名称" data-testid="store-source-new-name" className={cn(inputCls, 'w-[120px]')} />
        <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} aria-label="源地址" placeholder="https:// 或 file://" data-testid="store-source-new-baseurl" className={cn(inputCls, 'w-[200px]')} />
        <input
          ref={tokenRef}
          type="password"
          aria-label="源访问 token（可选，不回显）"
          placeholder="token（可选）"
          data-testid="store-source-new-token"
          className={cn(inputCls, 'w-[130px]')}
        />
        <button type="button" className={primaryBtn} data-testid="store-source-add" onClick={submit}>
          <Plus size={10} />
          新增源
        </button>
        <span className="text-[10px] text-fg-4">token 只单向送出，不回显；已配置的源仅显示徽标。</span>
      </div>
    </div>
  );
}

// ---------- 壳 ----------

export default function StoreTab() {
  const subTab = useStoreStore((s) => s.subTab);
  const setSubTab = useStoreStore((s) => s.setSubTab);
  const sourcesPanelOpen = useStoreStore((s) => s.sourcesPanelOpen);
  const toggleSourcesPanel = useStoreStore((s) => s.toggleSourcesPanel);
  const detail = useStoreStore((s) => s.detail);

  // 首屏:源清单 + 首页检索 + 已安装(发现页卡片要据此显示「已安装/更新到 x」)。
  useEffect(() => {
    const st = useStoreStore.getState();
    void st.loadSources();
    void st.runSearch();
    void st.loadInstalled();
  }, []);

  useEffect(() => {
    const st = useStoreStore.getState();
    if (subTab === 'installed') void st.loadInstalled();
    if (subTab === 'library') void st.loadLibrary();
  }, [subTab]);

  // 卸载即停轮询:定时器不得随组件消失而留在 event loop 里。
  useEffect(() => () => useStoreStore.getState().stopPolling(), []);

  const refresh = () => {
    const st = useStoreStore.getState();
    if (st.subTab === 'discover') {
      void st.runSearch();
      void st.loadInstalled();
    } else if (st.subTab === 'installed') {
      void st.loadInstalled();
    } else {
      void st.loadLibrary();
    }
    void st.loadSources();
  };

  return (
    <div className="flex h-full min-h-0 flex-col bg-shell-bg" data-testid="store-tab">
      {/* 顶部工具条 */}
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-edge px-3">
        <span className="flex items-center gap-0.5 rounded-md bg-shell-sunk p-0.5">
          {SUB_TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              data-testid={`store-subtab-${t.id}`}
              onClick={() => setSubTab(t.id)}
              className={cn(
                'rounded px-2.5 py-0.5 text-[11px] transition-colors',
                subTab === t.id ? 'bg-shell-panel text-fg shadow-sm' : 'text-fg-3 hover:text-fg-2',
              )}
            >
              {t.label}
            </button>
          ))}
        </span>
        <span className="flex-1" />
        <button
          type="button"
          aria-label="源管理"
          data-testid="store-sources-toggle"
          onClick={toggleSourcesPanel}
          className={cn(
            'flex h-[22px] items-center gap-1 rounded-md border border-edge px-2 text-[11px] transition-colors',
            sourcesPanelOpen ? 'bg-shell-active text-fg' : 'bg-shell-panel text-fg-2 hover:bg-shell-hover',
          )}
        >
          <Server size={11} />
          源管理
        </button>
        <button type="button" aria-label="刷新商店" data-testid="store-refresh" className={iconBtn} onClick={refresh}>
          <RefreshCw size={12} />
        </button>
      </div>

      {sourcesPanelOpen && <SourcesPanel />}

      <div className="flex min-h-0 flex-1">
        {subTab === 'discover' ? <DiscoverPage /> : subTab === 'installed' ? <InstalledPage /> : <LibraryPage />}
        {subTab === 'discover' && detail && <DetailPanel />}
      </div>
    </div>
  );
}
