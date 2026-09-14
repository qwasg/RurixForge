import { useEffect, useState } from 'react';
import { ChevronDown, ChevronRight, Plus, Search, Trash2 } from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  CATEGORY_META,
  CATEGORY_ORDER,
  entityCategory,
  type EntityCategory,
} from '@/lib/entityCategory';
import { useEditorStore, type EntityData } from '@/lib/editorStore';

/**
 * 场景层级(2026-08-24 壳右栏):按 角色/地图/交互 三分组展示,
 * 组内仍支持选中/改名/销毁;折叠态持久化。
 */

const iconBtn =
  'flex h-6 w-6 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2';

const COLLAPSE_KEY = 'forge:hierarchyGroups';

function loadCollapsePref(): Record<EntityCategory, boolean> {
  try {
    const raw = globalThis.localStorage?.getItem(COLLAPSE_KEY);
    if (!raw) return { role: false, map: false, interaction: false };
    const p = JSON.parse(raw) as Partial<Record<EntityCategory, boolean>>;
    return {
      role: p.role === true,
      map: p.map === true,
      interaction: p.interaction === true,
    };
  } catch {
    return { role: false, map: false, interaction: false };
  }
}

function persistCollapsePref(p: Record<EntityCategory, boolean>): void {
  try {
    globalThis.localStorage?.setItem(COLLAPSE_KEY, JSON.stringify(p));
  } catch {
    // 写不进静默
  }
}

export function entityParent(entity: EntityData): number | null {
  const value = entity.components.find((c) => c.enabled && c.type === 'Parent')?.props.entity;
  return typeof value === 'number' && Number.isSafeInteger(value) ? value : null;
}

/** Stable parent-first order; malformed cycles remain visible rather than hanging the UI. */
export function hierarchyRows(entities: EntityData[], collapsed: Set<number> = new Set()): Array<{ entity: EntityData; depth: number; hasChildren: boolean }> {
  const ids = new Set(entities.map((e) => e.id));
  const children = new Map<number, EntityData[]>();
  for (const e of entities) {
    const parent = entityParent(e);
    if (parent !== null && parent !== e.id && ids.has(parent)) children.set(parent, [...(children.get(parent) ?? []), e]);
  }
  const seen = new Set<number>();
  const out: Array<{ entity: EntityData; depth: number; hasChildren: boolean }> = [];
  const visit = (e: EntityData, depth: number, hidden: boolean) => {
    if (seen.has(e.id)) return;
    seen.add(e.id);
    const kids = children.get(e.id) ?? [];
    if (!hidden) out.push({ entity: e, depth, hasChildren: kids.length > 0 });
    for (const child of kids) visit(child, Math.min(depth + 1, 64), hidden || collapsed.has(e.id));
  };
  for (const e of entities) if (!ids.has(entityParent(e) ?? -1) || entityParent(e) === e.id) visit(e, 0, false);
  for (const e of entities) if (!seen.has(e.id)) visit(e, 0, false);
  return out;
}

function HierarchyRow({ entity, depth = 0, expanded = true, hasChildren = false, onToggle }: { entity: EntityData; depth?: number; expanded?: boolean; hasChildren?: boolean; onToggle?: () => void }) {
  const selectedId = useEditorStore((s) => s.selectedId);
  const selectEntity = useEditorStore((s) => s.selectEntity);
  const renameEntity = useEditorStore((s) => s.renameEntity);
  const destroyEntity = useEditorStore((s) => s.destroyEntity);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(entity.name);
  const cat = entityCategory(entity);
  const CatIcon = CATEGORY_META[cat].icon;

  const commit = () => {
    setEditing(false);
    const name = draft.trim();
    if (name !== '' && name !== entity.name) void renameEntity(entity.id, name);
    else setDraft(entity.name);
  };

  return (
    <div
      role="button"
      tabIndex={0}
      title={entity.name}
      data-testid={`hierarchy-row-${entity.id}`}
      data-category={cat}
      data-depth={depth}
      style={{ paddingLeft: 8 + depth * 12 }}
      onClick={() => selectEntity(entity.id)}
      onDoubleClick={() => {
        setDraft(entity.name);
        setEditing(true);
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') selectEntity(entity.id);
      }}
      className={cn(
        'group/entity flex w-full cursor-pointer items-center gap-1 rounded-md px-2 py-[3px] text-sm text-fg-2 transition-colors hover:bg-shell-hover',
        selectedId === entity.id && 'bg-acc-bg text-fg shadow-[inset_2px_0_0_0_var(--accent)]',
      )}
    >
      {hasChildren && <button type="button" aria-label={`${expanded ? '折叠' : '展开'} ${entity.name}`} aria-expanded={expanded}
        onClick={(e) => { e.stopPropagation(); onToggle?.(); }} className="shrink-0 text-fg-4">
        {expanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
      </button>}
      <CatIcon size={11} className={cn('shrink-0', CATEGORY_META[cat].headerClass)} strokeWidth={1.8} />
      {editing ? (
        <input
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commit();
            if (e.key === 'Escape') {
              setDraft(entity.name);
              setEditing(false);
            }
          }}
          onClick={(e) => e.stopPropagation()}
          className="min-w-0 flex-1 rounded border border-edge bg-shell-input px-1 py-px text-sm text-fg outline-none"
        />
      ) : (
        <span className="min-w-0 flex-1 truncate">{entity.name}</span>
      )}
      <span className="shrink-0 text-2xs text-fg-4">#{entity.id}</span>
      <button
        type="button"
        title="Destroy entity"
        className="hidden h-[18px] w-[18px] shrink-0 place-items-center rounded text-fg-4 hover:bg-shell-active hover:text-fg-2 group-hover/entity:grid"
        onClick={(e) => {
          e.stopPropagation();
          void destroyEntity(entity.id);
        }}
      >
        <Trash2 size={12} strokeWidth={1.8} />
      </button>
    </div>
  );
}

function CategoryGroup({
  category,
  entities,
  collapsed,
  onToggle,
}: {
  category: EntityCategory;
  entities: EntityData[];
  collapsed: boolean;
  onToggle: () => void;
}) {
  const meta = CATEGORY_META[category];
  const Icon = meta.icon;
  const [closedNodes, setClosedNodes] = useState(new Set<number>());

  return (
    <div className="mb-1" data-testid={`hierarchy-group-${category}`}>
      <button
        type="button"
        onClick={onToggle}
        className="flex w-full items-center gap-1 rounded-md px-1.5 py-1 text-left text-xs font-medium hover:bg-shell-hover"
      >
        {collapsed ? (
          <ChevronRight size={12} className="shrink-0 text-fg-4" />
        ) : (
          <ChevronDown size={12} className="shrink-0 text-fg-4" />
        )}
        <Icon size={12} className={cn('shrink-0', meta.headerClass)} strokeWidth={1.8} />
        <span className={cn('min-w-0 flex-1 truncate', meta.headerClass)}>{meta.label}</span>
        <span className="shrink-0 rounded-full bg-shell-sunk px-1.5 py-px text-2xs text-fg-4">{entities.length}</span>
      </button>
      {!collapsed && (
        <div className="pl-1">
          {hierarchyRows(entities, closedNodes).map(({ entity, depth, hasChildren }) => (
            <HierarchyRow key={entity.id} entity={entity} depth={depth} hasChildren={hasChildren} expanded={!closedNodes.has(entity.id)}
              onToggle={() => setClosedNodes((old) => { const next = new Set(old); if (next.has(entity.id)) next.delete(entity.id); else next.add(entity.id); return next; })} />
          ))}
          {entities.length === 0 && <p className="px-2 py-1 text-2xs text-fg-4">暂无{meta.label}实体</p>}
        </div>
      )}
    </div>
  );
}

export default function HierarchyPanel() {
  const entities = useEditorStore((s) => s.entities);
  const createEntity = useEditorStore((s) => s.createEntity);
  const loadEntities = useEditorStore((s) => s.loadEntities);
  const [filter, setFilter] = useState('');
  const [collapsed, setCollapsed] = useState(loadCollapsePref);

  const q = filter.trim().toLowerCase();
  const matches = new Set(entities.filter((e) => e.name.toLowerCase().includes(q)).map((e) => e.id));
  if (q) for (const e of entities.filter((e) => matches.has(e.id))) {
    let parent = entityParent(e);
    const seen = new Set<number>();
    while (parent !== null && !seen.has(parent)) {
      seen.add(parent); matches.add(parent);
      const found = entities.find((item) => item.id === parent);
      parent = found ? entityParent(found) : null;
    }
  }
  const filtered = q === '' ? entities : entities.filter((e) => matches.has(e.id));

  const grouped = CATEGORY_ORDER.reduce(
    (acc, cat) => {
      acc[cat] = filtered.filter((e) => entityCategory(e) === cat);
      return acc;
    },
    {} as Record<EntityCategory, EntityData[]>,
  );

  useEffect(() => {
    if (useEditorStore.getState().entities.length === 0) void loadEntities();
  }, [loadEntities]);

  const toggleGroup = (cat: EntityCategory) => {
    setCollapsed((prev) => {
      const next = { ...prev, [cat]: !prev[cat] };
      persistCollapsePref(next);
      return next;
    });
  };

  return (
    <section className="flex min-h-0 flex-1 flex-col" aria-label="Hierarchy" data-testid="hierarchy-panel">
      <div className="flex shrink-0 items-center justify-between px-2 pb-1 pt-2">
        <span className="text-2xs text-fg-4">层级 · 角色 / 地图 / 交互</span>
        <button type="button" title="Create entity" className={iconBtn} onClick={() => void createEntity()}>
          <Plus size={13} strokeWidth={1.8} />
        </button>
      </div>
      <div className="shrink-0 px-2 pb-1">
        <div className="flex items-center gap-1.5 rounded-md border border-edge bg-shell-input px-2 py-1">
          <Search size={12} className="shrink-0 text-fg-4" />
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter entities..."
            className="min-w-0 flex-1 bg-transparent text-xs text-fg outline-none placeholder:text-fg-4"
          />
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-1 pb-1">
        {CATEGORY_ORDER.map((cat) => (
          <CategoryGroup
            key={cat}
            category={cat}
            entities={grouped[cat]}
            collapsed={collapsed[cat]}
            onToggle={() => toggleGroup(cat)}
          />
        ))}
        {filtered.length === 0 && (
          <p className="px-2 pt-2 text-xs text-fg-4">
            {entities.length === 0 ? '场景为空,点 + 新建实体' : '无匹配实体'}
          </p>
        )}
      </div>
    </section>
  );
}
