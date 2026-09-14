import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { ChevronDown, ChevronRight, ExternalLink, X, Zap } from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  CATEGORY_META,
  CATEGORY_ORDER,
  assetRefIcon,
  entityCategory,
  eventNodeLabel,
  extractAssetRefs,
  type EntityCategory,
} from '@/lib/entityCategory';
import { useAssetStore } from '@/lib/assetStore';
import { useEditorStore } from '@/lib/editorStore';
import { useGraphStore, type GraphDoc } from '@/lib/graphStore';
import TemplateControls from './TemplateControls';

const iconBtn =
  'flex h-6 w-6 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-fg-3';

/** 数字单元格:失焦/回车提交;外部值变化时回填 */
function NumCell({ value, onCommit }: { value: number; onCommit: (v: number) => void }) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);

  const commit = () => {
    const n = Number(draft);
    if (draft.trim() === '' || Number.isNaN(n)) setDraft(String(value));
    else if (n !== value) onCommit(n);
  };

  return (
    <input
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
        if (e.key === 'Escape') setDraft(String(value));
      }}
      className="w-full min-w-0 rounded border border-edge bg-shell-input px-1 py-px font-mono text-2xs text-fg outline-none focus:border-fg-4"
    />
  );
}

function VecRow({
  label,
  values,
  labels,
  onCommit,
}: {
  label: string;
  values: number[];
  labels: string[];
  onCommit: (next: number[]) => void;
}) {
  return (
    <div className="flex items-center gap-1 px-2 py-0.5">
      <span className="w-[62px] shrink-0 text-2xs text-fg-3">{label}</span>
      <span className="flex min-w-0 flex-1 items-center gap-0.5">
        {values.map((v, i) => (
          <span key={i} className="flex min-w-0 flex-1 items-center gap-0.5">
            <span className="text-2xs text-fg-4">{labels[i]}</span>
            <NumCell
              value={v}
              onCommit={(n) => {
                const next = [...values];
                next[i] = n;
                onCommit(next);
              }}
            />
          </span>
        ))}
      </span>
    </div>
  );
}

function Section({
  title,
  defaultOpen = true,
  children,
}: {
  title: string;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="border-t border-edge py-1">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-1 px-2 py-0.5 text-left"
      >
        {open ? <ChevronDown size={11} className="text-fg-4" /> : <ChevronRight size={11} className="text-fg-4" />}
        <span className="text-2xs font-medium text-fg-2">{title}</span>
      </button>
      {open && children}
    </div>
  );
}

function graphEventNodes(graph: GraphDoc | null): Array<{ id: string; type: string; label: string }> {
  if (!graph) return [];
  return graph.nodes
    .filter((n) => n.type.startsWith('event.'))
    .map((n) => ({ id: n.id, type: n.type, label: eventNodeLabel(n.type) }));
}

/** Scratch 式实体卡片:分类 + 素材 + 交互事件 + 属性。 */
export default function EntityInspectorPanel() {
  const entities = useEditorStore((s) => s.entities);
  const selectedId = useEditorStore((s) => s.selectedId);
  const renameEntity = useEditorStore((s) => s.renameEntity);
  const setTransform = useEditorStore((s) => s.setTransform);
  const setCategory = useEditorStore((s) => s.setCategory);
  const componentTypes = useEditorStore((s) => s.componentTypes);
  const loadComponentTypes = useEditorStore((s) => s.loadComponentTypes);
  const addComponent = useEditorStore((s) => s.addComponent);
  const removeComponent = useEditorStore((s) => s.removeComponent);
  const setComponentEnabled = useEditorStore((s) => s.setComponentEnabled);
  const setCenterTab = useEditorStore((s) => s.setCenterTab);
  const toggleEditorPane = useEditorStore((s) => s.toggleEditorPane);
  const editorPanes = useEditorStore((s) => s.editorPanes);

  const assetItems = useAssetStore((s) => s.items);
  const focusPath = useAssetStore((s) => s.focusPath);
  const loadAssets = useAssetStore((s) => s.load);

  const loadByPath = useGraphStore((s) => s.loadByPath);
  const cardGraph = useGraphStore((s) => s.graph);
  const cardGraphPath = useGraphStore((s) => s.graphPath);
  const cardGraphLoading = useGraphStore((s) => s.loading);

  const entity = entities.find((e) => e.id === selectedId) ?? null;
  const [nameDraft, setNameDraft] = useState('');
  const category = entity ? entityCategory(entity) : null;

  const scriptGraphRef = useMemo(() => {
    const script = entity?.components.find((c) => c.type === 'Script' && c.enabled);
    const ref = script?.props?.graphRef;
    return typeof ref === 'string' && ref.trim() !== '' ? ref : null;
  }, [entity]);

  const assetRefs = useMemo(() => (entity ? extractAssetRefs(entity) : []), [entity]);
  const trigger = entity?.components.find((c) => c.type === 'Trigger' && c.enabled);

  useEffect(() => setNameDraft(entity?.name ?? ''), [entity?.id, entity?.name]);
  useEffect(() => {
    if (componentTypes.length === 0) void loadComponentTypes();
  }, [componentTypes.length, loadComponentTypes]);

  useEffect(() => {
    if (scriptGraphRef) void loadByPath(scriptGraphRef);
  }, [scriptGraphRef, loadByPath]);

  useEffect(() => {
    if (assetItems.length === 0) void loadAssets();
  }, [assetItems.length, loadAssets]);

  const commitName = () => {
    const name = nameDraft.trim();
    if (entity && name !== '' && name !== entity.name) void renameEntity(entity.id, name);
    else setNameDraft(entity?.name ?? '');
  };

  const addable = componentTypes.filter((t) => !entity?.components.some((c) => c.type === t.name));
  const events = scriptGraphRef && cardGraphPath === scriptGraphRef ? graphEventNodes(cardGraph) : [];

  const openAsset = (path: string) => {
    if (!editorPanes.assets) toggleEditorPane('assets');
    focusPath(path);
  };

  const openNodeGraph = () => {
    setCenterTab('nodegraph');
  };

  return (
    <div
      className="flex min-h-0 flex-1 flex-col bg-shell-sidebar"
      aria-label="Inspector"
      data-testid="editor-pane-inspector"
    >
      {!entity ? (
        <p className="px-3 pt-2 text-xs text-fg-4">在视口或层级中选中实体,查看素材与交互事件</p>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto pb-2">
          {/* 头部:名称 + 分类 chip */}
          <div className="space-y-1.5 px-2.5 pb-2 pt-1">
            <div className="flex items-center justify-between gap-2">
              <span className="font-mono text-2xs text-fg-3">Entity #{entity.id}</span>
              {category && (
                <span
                  className={cn(
                    'rounded-full border px-2 py-px text-2xs font-medium',
                    CATEGORY_META[category].chipClass,
                  )}
                  data-testid="entity-category-chip"
                >
                  {CATEGORY_META[category].label}
                </span>
              )}
            </div>
            <input
              value={nameDraft}
              onChange={(e) => setNameDraft(e.target.value)}
              onBlur={commitName}
              onKeyDown={(e) => {
                if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
              }}
              className="w-full rounded-md border border-edge bg-shell-input px-2 py-1 text-sm text-fg outline-none focus:border-fg-4"
            />
            <select
              value={category ?? 'map'}
              title="分类"
              data-testid="entity-category-select"
              onChange={(e) => void setCategory(entity.id, e.target.value as EntityCategory)}
              className="w-full rounded-md border border-edge bg-shell-input px-2 py-1 text-xs text-fg-2 outline-none"
            >
              {CATEGORY_ORDER.map((c) => (
                <option key={c} value={c}>
                  {CATEGORY_META[c].label}
                </option>
              ))}
            </select>
          </div>

          {/* 素材区 */}
          <TemplateControls key={entity.id} entity={entity} />
          <Section title="素材" defaultOpen>
            <div className="space-y-1 px-2 pb-1">
              {assetRefs.length === 0 && <p className="text-2xs text-fg-4">无关联素材</p>}
              {assetRefs.map((ref) => {
                const Icon = assetRefIcon(ref.kind);
                const matched = assetItems.find(
                  (a) => a.path === ref.path || a.path.endsWith(`/${ref.path}`) || a.guid === ref.path,
                );
                return (
                  <button
                    key={ref.key}
                    type="button"
                    data-testid={`entity-asset-${ref.kind}`}
                    onClick={() => openAsset(matched?.path ?? ref.path)}
                    className="flex w-full items-center gap-2 rounded-md border border-edge bg-shell-panel px-2 py-1 text-left hover:bg-shell-hover"
                  >
                    <Icon size={13} className="shrink-0 text-fg-3" strokeWidth={1.6} />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-xs text-fg-2">{ref.label}</span>
                      <span className="block truncate font-mono text-2xs text-fg-4">{matched?.path ?? ref.path}</span>
                    </span>
                    <ExternalLink size={11} className="shrink-0 text-fg-4" />
                  </button>
                );
              })}
            </div>
          </Section>

          {/* 交互事件区 */}
          <Section title="交互事件" defaultOpen>
            <div className="space-y-1 px-2 pb-1">
              {trigger && (
                <div
                  className="flex items-start gap-2 rounded-md border border-edge bg-shell-panel px-2 py-1"
                  data-testid="entity-trigger-info"
                >
                  <Zap size={13} className="mt-0.5 shrink-0 text-warn" strokeWidth={1.6} />
                  <div className="min-w-0 flex-1">
                    <p className="text-xs text-fg-2">触发区 · {String(trigger.props.kind ?? 'box')}</p>
                    <p className="truncate font-mono text-2xs text-fg-4">
                      extents: {JSON.stringify(trigger.props.extents ?? [])}
                    </p>
                  </div>
                </div>
              )}
              {scriptGraphRef && (
                <div className="rounded-md border border-edge bg-shell-panel px-2 py-1">
                  <div className="flex items-center justify-between gap-1">
                    <p className="truncate font-mono text-2xs text-fg-3" title={scriptGraphRef}>
                      {scriptGraphRef}
                    </p>
                    <button
                      type="button"
                      title="打开节点图"
                      className="shrink-0 text-2xs text-acc hover:underline"
                      onClick={openNodeGraph}
                    >
                      查看图
                    </button>
                  </div>
                  {cardGraphLoading && <p className="pt-1 text-2xs text-fg-4">加载事件…</p>}
                  {!cardGraphLoading && events.length === 0 && (
                    <p className="pt-1 text-2xs text-fg-4">图中无 event.* 节点</p>
                  )}
                  {events.map((ev) => (
                    <button
                      key={ev.id}
                      type="button"
                      data-testid={`entity-event-${ev.id}`}
                      onClick={openNodeGraph}
                      className="mt-1 flex w-full items-center gap-2 rounded px-1.5 py-0.5 text-left hover:bg-shell-hover"
                    >
                      <span className="rounded bg-warn/15 px-1.5 py-px text-2xs text-warn">{ev.label}</span>
                      <span className="truncate font-mono text-2xs text-fg-4">{ev.type}</span>
                    </button>
                  ))}
                </div>
              )}
              {!trigger && !scriptGraphRef && <p className="text-2xs text-fg-4">无交互事件</p>}
            </div>
          </Section>

          {/* 属性区 */}
          <Section title="属性">
            <div className="border-t border-edge py-1">
              <p className="px-2 py-0.5 text-2xs font-medium text-fg-2">Transform</p>
              <VecRow
                label="Position"
                values={entity.transform.translation}
                labels={['x', 'y', 'z']}
                onCommit={(next) => void setTransform(entity.id, { translation: next })}
              />
              <VecRow
                label="Rotation"
                values={entity.transform.rotation}
                labels={['x', 'y', 'z', 'w']}
                onCommit={(next) => void setTransform(entity.id, { rotation: next })}
              />
              <VecRow
                label="Scale"
                values={entity.transform.scale}
                labels={['x', 'y', 'z']}
                onCommit={(next) => void setTransform(entity.id, { scale: next })}
              />
            </div>

            {entity.components.filter((c) => !['PrefabInstance', 'ModelNode', 'Parent'].includes(c.type)).map((c) => (
              <div key={c.type} className="border-t border-edge py-1">
                <div className="flex items-center gap-1.5 px-2 py-0.5">
                  <input
                    type="checkbox"
                    checked={c.enabled}
                    title="enabled"
                    onChange={(e) => void setComponentEnabled(entity.id, c.type, e.target.checked)}
                    className="h-3 w-3 shrink-0 accent-[var(--accent)]"
                  />
                  <span className="min-w-0 flex-1 truncate text-2xs font-medium text-fg-2">{c.type}</span>
                  <button
                    type="button"
                    title="Remove component"
                    className={iconBtn}
                    onClick={() => void removeComponent(entity.id, c.type)}
                  >
                    <X size={11} strokeWidth={1.8} />
                  </button>
                </div>
                <div className="px-2 pl-7 font-mono text-2xs text-fg-3">
                  {Object.keys(c.props).length === 0 && <p className="text-fg-4">(无属性)</p>}
                  {Object.entries(c.props).map(([k, v]) => (
                    <p key={k} className="truncate py-px" title={JSON.stringify(v)}>
                      <span className="text-fg-4">{k}</span>: {JSON.stringify(v)}
                    </p>
                  ))}
                </div>
              </div>
            ))}

            <div className="border-t border-edge px-2.5 py-2">
              <select
                value=""
                title="Add Component"
                onChange={(e) => {
                  const t = e.target.value;
                  if (t !== '') void addComponent(entity.id, t);
                }}
                className="w-full rounded-md border border-edge bg-shell-input px-2 py-1 text-xs text-fg-2 outline-none"
              >
                <option value="" disabled>
                  + Add Component
                </option>
                {addable.map((t) => (
                  <option key={t.name} value={t.name}>
                    {t.name}
                  </option>
                ))}
              </select>
            </div>
          </Section>
        </div>
      )}
    </div>
  );
}
