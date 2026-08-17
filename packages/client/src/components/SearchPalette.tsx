import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react';
import {
  ArrowLeft,
  ArrowRight,
  Bot,
  Code,
  Columns2,
  Contrast,
  FileText,
  FolderOpen,
  ListChecks,
  Monitor,
  Moon,
  Pin,
  RefreshCw,
  Rows2,
  Search,
  Sun,
  type LucideIcon,
} from 'lucide-react';
import { useAppStore } from '@/lib/store';
import { PALETTE_ROWS, type PaletteRow } from '@/lib/mock';
import { cn } from '@/lib/cn';

/* Ctrl+K 命令面板:复刻 Cursor Agent 的搜索弹层(布局视频 00:06 / 00:12)。 */

const TABS = ['All', 'Agents', 'Files', 'Actions', 'Settings'] as const;
type Tab = (typeof TABS)[number];

const PLACEHOLDERS: Record<Tab, string> = {
  All: 'Search agents, files, actions...',
  Agents: 'Search agents...',
  Files: 'Search files...',
  Actions: 'Search actions...',
  Settings: 'Search settings...',
};

/** All  tab 下各分组的出现顺序与小组标题 */
const GROUP_ORDER: PaletteRow['group'][] = ['Agents', 'Files', 'Settings', 'Actions'];
const GROUP_TITLES: Record<PaletteRow['group'], string> = {
  Agents: 'Recent Agents',
  Files: 'Files',
  Settings: 'Settings',
  Actions: 'Actions',
};

/** 按 label 细化的行图标(视频里每个 action 图标不同),兜底按分组 */
const LABEL_ICONS: Record<string, LucideIcon> = {
  'Reload Window': RefreshCw,
  'Toggle Developer Tools': Code,
  'Go Back': ArrowLeft,
  'Go Forward': ArrowRight,
  'Open Conversation Logs Folder': FolderOpen,
  'Split Tile Horizontally': Columns2,
  'Split Tile Vertically': Rows2,
  'Pin / Unpin Agent': Pin,
  'Search Agents': Search,
  'Plan Mode': ListChecks,
  'Agent Mode': Bot,
  'Light Theme': Sun,
  'Dark Theme': Moon,
  'System Theme': Monitor,
  'High Contrast Theme': Contrast,
};

const GROUP_ICONS: Record<PaletteRow['group'], LucideIcon> = {
  Agents: Bot,
  Files: FileText,
  Actions: ArrowRight,
  Settings: Sun,
};

function rowIcon(row: PaletteRow): LucideIcon {
  return LABEL_ICONS[row.label] ?? GROUP_ICONS[row.group];
}

export default function SearchPalette() {
  const paletteOpen = useAppStore((s) => s.paletteOpen);
  const setPaletteOpen = useAppStore((s) => s.setPaletteOpen);
  const openAgent = useAppStore((s) => s.openAgent);

  const [query, setQuery] = useState('');
  const [tab, setTab] = useState<Tab>('All');
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const close = () => setPaletteOpen(false);

  /* 每次打开时重置状态并聚焦输入框 */
  useEffect(() => {
    if (paletteOpen) {
      setQuery('');
      setTab('All');
      setActive(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [paletteOpen]);

  /* Esc 关闭(即使焦点不在输入框上,如刚点过 tab) */
  useEffect(() => {
    if (!paletteOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        close();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paletteOpen]);

  /* 当前 tab + 关键字过滤 */
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return PALETTE_ROWS.filter(
      (r) =>
        (tab === 'All' || r.group === tab) &&
        (!q ||
          r.label.toLowerCase().includes(q) ||
          (r.workspace ?? '').toLowerCase().includes(q)),
    );
  }, [query, tab]);

  /* 分组展示:All 按组分段,单 tab 统一收在 'Suggested' 下 */
  const sections = useMemo(() => {
    const byGroup = new Map<PaletteRow['group'], PaletteRow[]>();
    for (const row of filtered) {
      const list = byGroup.get(row.group);
      if (list) list.push(row);
      else byGroup.set(row.group, [row]);
    }
    return GROUP_ORDER.filter((g) => byGroup.has(g)).map((g) => ({
      key: g,
      title: tab === 'All' ? GROUP_TITLES[g] : 'Suggested',
      rows: byGroup.get(g)!,
    }));
  }, [filtered, tab]);

  const flat = useMemo(() => sections.flatMap((s) => s.rows), [sections]);

  /* 过滤条件变化时高亮回顶并钳位 */
  useEffect(() => {
    setActive(0);
  }, [query, tab]);
  useEffect(() => {
    setActive((a) => Math.min(a, Math.max(flat.length - 1, 0)));
  }, [flat.length]);

  /* 键盘移动时让高亮行保持可见 */
  useEffect(() => {
    listRef.current
      ?.querySelector(`[data-idx="${active}"]`)
      ?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  const openRow = (row: PaletteRow | undefined) => {
    if (!row) return;
    if (row.group === 'Agents') openAgent(row.id); // openAgent 内部会关闭面板
    else close();
  };

  const moveTab = (delta: number) => {
    const i = TABS.indexOf(tab);
    setTab(TABS[(i + delta + TABS.length) % TABS.length]);
    inputRef.current?.focus();
  };

  const onInputKeyDown = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, flat.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      openRow(flat[active]);
    } else if (e.ctrlKey && e.key === '[') {
      e.preventDefault();
      moveTab(-1);
    } else if (e.ctrlKey && e.key === ']') {
      e.preventDefault();
      moveTab(1);
    }
  };

  if (!paletteOpen) return null;

  let idx = -1;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[24vh]"
      onClick={close}
    >
      <div
        className="w-[640px] max-w-[calc(100vw-64px)] overflow-hidden rounded-2xl bg-white shadow-pop"
        onClick={(e) => e.stopPropagation()}
      >
        {/* 输入框:左侧无图标 */}
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onInputKeyDown}
          placeholder={PLACEHOLDERS[tab]}
          spellCheck={false}
          className="w-full bg-transparent px-4 pb-2 pt-3.5 text-sm text-ink outline-none placeholder:text-muted-faint"
        />

        {/* tab 行 */}
        <div className="flex items-center gap-1 px-3 pb-2">
          {TABS.map((t) => (
            <button
              key={t}
              onClick={() => {
                setTab(t);
                inputRef.current?.focus();
              }}
              className={cn(
                'rounded-md px-2.5 py-1 text-xs transition-colors',
                t === tab
                  ? 'bg-panel-active text-ink'
                  : 'text-muted hover:bg-panel-hover',
              )}
            >
              {t}
            </button>
          ))}
        </div>

        {/* 结果列表 */}
        <div
          ref={listRef}
          className="h-[400px] overflow-y-auto border-t border-line-soft px-1.5 py-1.5"
        >
          {sections.length === 0 && (
            <div className="py-10 text-center text-xs text-muted-faint">
              No results found
            </div>
          )}
          {sections.map((section) => (
            <div key={section.key}>
              <div className="px-2.5 pb-1 pt-1.5 text-2xs text-muted-faint">
                {section.title}
              </div>
              {section.rows.map((row) => {
                idx += 1;
                const i = idx;
                const Icon = rowIcon(row);
                return (
                  <button
                    key={row.id}
                    data-idx={i}
                    onMouseEnter={() => setActive(i)}
                    onClick={() => openRow(row)}
                    className={cn(
                      'flex w-full items-center gap-2.5 rounded-md px-2.5 py-[7px] text-left',
                      i === active && 'bg-panel-hover',
                    )}
                  >
                    <Icon
                      size={15}
                      strokeWidth={1.75}
                      className="shrink-0 text-muted"
                    />
                    <span className="truncate text-sm text-ink-soft">
                      {row.label}
                    </span>
                    <span className="ml-auto flex shrink-0 items-center gap-2 text-xs text-muted-faint">
                      {row.workspace && <span>{row.workspace}</span>}
                      {row.ago && <span>{row.ago}</span>}
                      {row.shortcut && <span>{row.shortcut}</span>}
                    </span>
                  </button>
                );
              })}
            </div>
          ))}
        </div>

        {/* footer 快捷键提示 */}
        <div className="flex items-center gap-3 border-t border-line-soft px-4 py-2 text-2xs text-muted-faint">
          <span>↑↓ Select</span>
          <span>⏎ Open</span>
          <span>Ctrl+[ or Ctrl+] Change Filter</span>
        </div>
      </div>
    </div>
  );
}
