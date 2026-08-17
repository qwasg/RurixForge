import { useMemo, useState } from 'react';
import {
  ChevronDown,
  MoreHorizontal,
  Plus,
  Search,
  SlidersHorizontal,
  User,
  Zap,
} from 'lucide-react';
import { CUSTOMIZE_ITEMS, CUSTOMIZE_TABS } from '@/lib/mock';
import type { CustomizeTab } from '@/lib/mock';
import { cn } from '@/lib/cn';

/** Customize 页:搜索 + tab 行 + User 小节列表;MCPs tab 顶部多一张横幅卡。 */
export default function CustomizeView() {
  const [tab, setTab] = useState<CustomizeTab>('MCPs');
  const [query, setQuery] = useState('');

  const items = CUSTOMIZE_ITEMS[tab];
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter(
      (i) => i.name.toLowerCase().includes(q) || i.description.toLowerCase().includes(q),
    );
  }, [items, query]);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-[880px] px-8 pb-16 pt-6">
        {/* 顶部搜索 + Browse Marketplace */}
        <div className="flex items-center gap-3">
          <div className="relative flex-1">
            <Search
              size={14}
              className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-muted-faint"
            />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === 'Escape' && setQuery('')}
              placeholder={`Search ${tab} for Kai Li...`}
              className="w-full rounded-full border border-line bg-white py-2 pl-9 pr-4 text-sm text-ink outline-none transition-colors placeholder:text-muted-faint focus:border-muted-faint"
            />
          </div>
          <button type="button" className="btn-primary shrink-0">
            Browse Marketplace
          </button>
        </div>

        {/* tab 行 */}
        <div className="mt-4 flex items-center gap-1">
          <button type="button" className={cn('chip', 'mr-1 text-ink-soft')}>
            <User size={13} />
            Kai Li
            <ChevronDown size={12} className="text-muted-faint" />
          </button>
          {CUSTOMIZE_TABS.map((t) => (
            <button
              key={t}
              type="button"
              onClick={() => setTab(t)}
              className={cn(
                'rounded-full px-3 py-1 text-xs transition-colors',
                tab === t ? 'bg-panel-hover font-medium text-ink' : 'text-muted hover:bg-panel-hover',
              )}
            >
              {t}
            </button>
          ))}
          <button
            type="button"
            className="ml-auto rounded-full p-1.5 text-muted transition-colors hover:bg-panel-hover hover:text-ink"
          >
            <SlidersHorizontal size={13} />
          </button>
        </div>

        {/* 小节标题 */}
        <div className="mt-6 flex items-center justify-between">
          <span className="text-xs text-muted">User {items.length}</span>
          <button
            type="button"
            className="inline-flex items-center gap-0.5 rounded-full px-2 py-1 text-xs text-muted transition-colors hover:bg-panel-hover hover:text-ink"
          >
            <Plus size={12} />
            New
          </button>
        </div>

        {/* MCPs 横幅卡 */}
        {tab === 'MCPs' && (
          <div className="mt-3 flex flex-col items-center rounded-xl bg-panel px-6 py-8 text-center">
            <p className="text-sm font-semibold text-ink">Connect External Tools with MCP</p>
            <p className="mt-1.5 max-w-[440px] text-xs leading-5 text-muted">
              Use Model Context Protocol servers connect Cursor to external tools and data sources
              like Linear, Figma, and Notion.
            </p>
            <div className="mt-4 flex items-center gap-2">
              <button type="button" className="btn-ghost">
                <Plus size={12} />
                New
              </button>
              <button type="button" className="btn-ghost">
                Documentation
              </button>
            </div>
          </div>
        )}

        {/* 列表行 */}
        <div className="mt-2">
          {filtered.map((item) => (
            <div
              key={item.id}
              className="group flex items-center gap-3 rounded-lg px-3 py-2.5 transition-colors hover:bg-panel-hover"
            >
              <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-panel-hover text-ink-soft">
                <Zap size={14} strokeWidth={1.75} />
              </span>
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-semibold text-ink">{item.name}</p>
                <p className="truncate text-xs text-muted">{item.description}</p>
              </div>
              <button
                type="button"
                className="shrink-0 rounded-full p-1 text-muted-faint transition-colors hover:bg-white hover:text-ink"
              >
                <MoreHorizontal size={15} />
              </button>
            </div>
          ))}
          {filtered.length === 0 && (
            <p className="py-10 text-center text-xs text-muted-faint">No results found</p>
          )}
        </div>
      </div>
    </div>
  );
}
