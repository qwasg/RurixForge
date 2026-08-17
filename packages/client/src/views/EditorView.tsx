import { useEffect, useState } from 'react';
import {
  ChevronRight,
  FolderOpen,
  MessageSquare,
  Move,
  Pause,
  Play,
  Plus,
  Redo2,
  RefreshCw,
  Rotate3d,
  Save,
  Scaling,
  Search,
  Square,
  StepForward,
  Trash2,
  Undo2,
  X,
} from 'lucide-react';
import { cn } from '@/lib/cn';
import { ViewportCanvas } from '@/components/editor/ViewportCanvas';
import AssetsPanel from '@/components/editor/AssetsPanel';
import NodeGraphView from '@/components/editor/NodeGraphView';
import { useGraphStore } from '@/lib/graphStore';
import {
  COMPOSER_MODES,
  useEditorStore,
  type ComposerMode,
  type EntityData,
  type GizmoMode,
  type PlayState,
  type WorkbenchTab,
} from '@/lib/editorStore';

/**
 * 编辑器视图:07 §1 七区骨架。
 * A Hierarchy / B Assets(F2 占位)/ C Viewport(+G NodeGraph 同位页签)/
 * D Inspector / E Workbench / F Chat(可折叠右 dock)。
 */

const iconBtn =
  'flex h-6 w-6 items-center justify-center rounded-md text-muted transition-colors hover:bg-panel-hover hover:text-ink-soft disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-muted';

// ---------- A Hierarchy ----------

function HierarchyRow({ entity }: { entity: EntityData }) {
  const selectedId = useEditorStore((s) => s.selectedId);
  const selectEntity = useEditorStore((s) => s.selectEntity);
  const renameEntity = useEditorStore((s) => s.renameEntity);
  const destroyEntity = useEditorStore((s) => s.destroyEntity);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(entity.name);

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
      onClick={() => selectEntity(entity.id)}
      onDoubleClick={() => {
        setDraft(entity.name);
        setEditing(true);
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') selectEntity(entity.id);
      }}
      className={cn(
        'group/entity flex w-full cursor-pointer items-center gap-1 rounded-md px-2 py-[3px] text-sm text-ink-soft transition-colors hover:bg-panel-hover',
        selectedId === entity.id && 'bg-panel-active',
      )}
    >
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
          className="min-w-0 flex-1 rounded border border-line bg-white px-1 py-px text-sm text-ink outline-none"
        />
      ) : (
        <span className="min-w-0 flex-1 truncate">{entity.name}</span>
      )}
      <span className="shrink-0 text-2xs text-muted-faint">#{entity.id}</span>
      <button
        type="button"
        title="Destroy entity"
        className="hidden h-[18px] w-[18px] shrink-0 place-items-center rounded text-muted-faint hover:bg-panel-active hover:text-muted group-hover/entity:grid"
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

function HierarchyPanel() {
  const entities = useEditorStore((s) => s.entities);
  const createEntity = useEditorStore((s) => s.createEntity);
  const [filter, setFilter] = useState('');
  const shown = entities.filter((e) => e.name.toLowerCase().includes(filter.trim().toLowerCase()));

  return (
    <section className="flex min-h-0 flex-1 flex-col" aria-label="Hierarchy">
      <div className="flex shrink-0 items-center justify-between px-2 pb-1 pt-2">
        <span className="text-2xs text-muted-faint">Hierarchy</span>
        <button
          type="button"
          title="Create entity"
          className={iconBtn}
          onClick={() => void createEntity()}
        >
          <Plus size={13} strokeWidth={1.8} />
        </button>
      </div>
      <div className="shrink-0 px-2 pb-1">
        <div className="flex items-center gap-1.5 rounded-md border border-line bg-white px-2 py-1">
          <Search size={12} className="shrink-0 text-muted-faint" />
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter entities..."
            className="min-w-0 flex-1 bg-transparent text-xs text-ink outline-none placeholder:text-muted-faint"
          />
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-1 pb-1">
        {shown.map((e) => (
          <HierarchyRow key={e.id} entity={e} />
        ))}
        {shown.length === 0 && (
          <p className="px-2 pt-2 text-xs text-muted-faint">
            {entities.length === 0 ? '场景为空,点 + 新建实体' : '无匹配实体'}
          </p>
        )}
      </div>
    </section>
  );
}

// ---------- B Assets(F2 wave.3 全量) ----------
// AssetsPanel 已迁至 components/editor/AssetsPanel.tsx:网格/列表、类型过滤、搜索、
// 拖拽实例化、右键六菜单、buildState 角标。

// ---------- C Viewport / G NodeGraph ----------

const GIZMOS: Array<{ mode: GizmoMode; title: string; icon: typeof Move }> = [
  { mode: 'translate', title: 'Move (W)', icon: Move },
  { mode: 'rotate', title: 'Rotate (E)', icon: Rotate3d },
  { mode: 'scale', title: 'Scale (R)', icon: Scaling },
];

function PlayControls() {
  const playState = useEditorStore((s) => s.playState);
  const playEnter = useEditorStore((s) => s.playEnter);
  const playPause = useEditorStore((s) => s.playPause);
  const playResume = useEditorStore((s) => s.playResume);
  const playStep = useEditorStore((s) => s.playStep);
  const playExit = useEditorStore((s) => s.playExit);

  return (
    <span className="flex items-center gap-0.5">
      <button
        type="button"
        title="Play"
        className={cn(iconBtn, playState !== 'edit' && playState !== 'play_paused' && 'opacity-40')}
        disabled={playState === 'play_running'}
        onClick={() => void (playState === 'play_paused' ? playResume() : playEnter())}
      >
        <Play size={13} strokeWidth={1.8} />
      </button>
      <button
        type="button"
        title="Pause"
        className={iconBtn}
        disabled={playState !== 'play_running'}
        onClick={() => void playPause()}
      >
        <Pause size={13} strokeWidth={1.8} />
      </button>
      <button
        type="button"
        title="Step"
        className={iconBtn}
        disabled={playState !== 'play_paused'}
        onClick={() => void playStep()}
      >
        <StepForward size={13} strokeWidth={1.8} />
      </button>
      <button
        type="button"
        title="Stop (exit PIE)"
        className={iconBtn}
        disabled={playState === 'edit'}
        onClick={() => void playExit()}
      >
        <Square size={12} strokeWidth={1.8} />
      </button>
    </span>
  );
}

const PIE_DOT: Record<PlayState, string> = {
  edit: 'bg-muted-faint',
  play_running: 'bg-accent-green',
  play_paused: 'bg-accent-blue',
};

function ViewportPanel() {
  const centerTab = useEditorStore((s) => s.centerTab);
  const setCenterTab = useEditorStore((s) => s.setCenterTab);
  const gizmo = useEditorStore((s) => s.gizmo);
  const setGizmo = useEditorStore((s) => s.setGizmo);
  const playState = useEditorStore((s) => s.playState);
  const sceneName = useEditorStore((s) => s.sceneName);
  const chatOpen = useEditorStore((s) => s.chatOpen);
  const toggleChat = useEditorStore((s) => s.toggleChat);

  return (
    <section className="flex min-h-0 flex-1 flex-col" aria-label="Viewport">
      {/* 顶部工具条:Viewport/NodeGraph 页签 + gizmo + PIE 控制 + Chat 开关 */}
      <div className="flex shrink-0 items-center gap-2 border-b border-line-soft px-2 py-1">
        <span className="flex items-center gap-0.5 rounded-md bg-panel p-0.5">
          {(['viewport', 'nodegraph'] as const).map((t) => (
            <button
              key={t}
              type="button"
              onClick={() => setCenterTab(t)}
              className={cn(
                'rounded px-2 py-0.5 text-xs capitalize transition-colors',
                centerTab === t ? 'bg-white text-ink shadow-sm' : 'text-muted hover:text-ink-soft',
              )}
            >
              {t === 'viewport' ? 'Viewport' : 'NodeGraph'}
            </button>
          ))}
        </span>
        <span className="h-4 w-px bg-line" />
        <span className="flex items-center gap-0.5" aria-label="Gizmo">
          {GIZMOS.map(({ mode, title, icon: Icon }) => (
            <button
              key={mode}
              type="button"
              title={title}
              className={cn(iconBtn, gizmo === mode && 'bg-panel-active text-ink')}
              onClick={() => setGizmo(mode)}
            >
              <Icon size={13} strokeWidth={1.8} />
            </button>
          ))}
        </span>
        <span className="h-4 w-px bg-line" />
        <PlayControls />
        <span className="flex-1" />
        <button
          type="button"
          title="Toggle Chat"
          className={cn(iconBtn, chatOpen && 'bg-panel-active text-ink')}
          onClick={toggleChat}
        >
          <MessageSquare size={13} strokeWidth={1.8} />
        </button>
      </div>

      {centerTab === 'viewport' ? (
        <div className="relative min-h-0 flex-1 bg-ink">
          {/* GPU 场景实渲染帧 + 点选/相机/gizmo(wave.2,RD-F1-001 回填) */}
          <ViewportCanvas />
          {/* PIE 状态条(play_state 实测) */}
          <div className="absolute inset-x-0 bottom-0 flex items-center gap-2 bg-black/40 px-2 py-1">
            <span className={cn('h-1.5 w-1.5 rounded-full', PIE_DOT[playState])} />
            <span className="font-mono text-2xs text-white/70">{playState}</span>
            <span className="flex-1" />
            <span className="truncate text-2xs text-white/40">{sceneName || 'Untitled'}</span>
          </div>
        </div>
      ) : (
        /* G NodeGraph 同位页签(F4 wave.4):图查看/微调/保存 */
        <NodeGraphView />
      )}
    </section>
  );
}

// ---------- E Workbench ----------

const WORKBENCH_TABS: WorkbenchTab[] = ['console', 'problems', 'output', 'terminal', 'logs', 'metrics'];

function ConsoleBody() {
  const events = useEditorStore((s) => s.events);
  const lastError = useEditorStore((s) => s.lastError);
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-2 py-1 font-mono text-2xs text-ink-soft">
      {lastError && <p className="py-0.5 text-accent-blue">[client] {lastError}</p>}
      {events.length === 0 && <p className="py-1 text-muted-faint">暂无 host 事件</p>}
      {events.map((e, i) => {
        const { ts, event, ...rest } = e;
        return (
          <p key={i} className="truncate py-px" title={JSON.stringify(e)}>
            <span className="text-muted-faint">{typeof ts === 'string' ? ts : ''}</span>{' '}
            <span>{typeof event === 'string' ? event : 'event'}</span>{' '}
            <span className="text-muted">{JSON.stringify(rest)}</span>
          </p>
        );
      })}
    </div>
  );
}

function WorkbenchPanel() {
  const workbenchTab = useEditorStore((s) => s.workbenchTab);
  const setWorkbenchTab = useEditorStore((s) => s.setWorkbenchTab);
  const undo = useEditorStore((s) => s.undo);
  const redo = useEditorStore((s) => s.redo);
  const saveScene = useEditorStore((s) => s.saveScene);
  const loadScene = useEditorStore((s) => s.loadScene);
  const loadEvents = useEditorStore((s) => s.loadEvents);

  return (
    <section
      className="flex h-[190px] shrink-0 flex-col border-t border-line bg-white"
      aria-label="Workbench"
    >
      <div className="flex shrink-0 items-center gap-0.5 border-b border-line-soft px-2">
        {WORKBENCH_TABS.map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => setWorkbenchTab(t)}
            className={cn(
              'border-b-2 px-2 py-1.5 text-xs capitalize transition-colors',
              workbenchTab === t
                ? 'border-ink text-ink'
                : 'border-transparent text-muted hover:text-ink-soft',
            )}
          >
            {t}
          </button>
        ))}
        <span className="flex-1" />
        {/* 场景级操作:Undo/Redo/Save/Load */}
        <button type="button" title="Undo" className={iconBtn} onClick={() => void undo()}>
          <Undo2 size={13} strokeWidth={1.8} />
        </button>
        <button type="button" title="Redo" className={iconBtn} onClick={() => void redo()}>
          <Redo2 size={13} strokeWidth={1.8} />
        </button>
        <span className="mx-0.5 h-4 w-px bg-line" />
        <button type="button" title="Save Scene" className={iconBtn} onClick={() => void saveScene()}>
          <Save size={13} strokeWidth={1.8} />
        </button>
        <button type="button" title="Load Scene" className={iconBtn} onClick={() => void loadScene()}>
          <FolderOpen size={13} strokeWidth={1.8} />
        </button>
        {workbenchTab === 'console' && (
          <button
            type="button"
            title="Refresh events"
            className={iconBtn}
            onClick={() => void loadEvents()}
          >
            <RefreshCw size={12} strokeWidth={1.8} />
          </button>
        )}
      </div>
      {workbenchTab === 'console' ? (
        <ConsoleBody />
      ) : (
        <div className="flex flex-1 items-center justify-center">
          <p className="text-xs text-muted-faint">{workbenchTab} 占位(F 后续里程碑承接)</p>
        </div>
      )}
    </section>
  );
}

// ---------- D Inspector ----------

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
      className="w-full min-w-0 rounded border border-line bg-white px-1 py-px font-mono text-2xs text-ink outline-none focus:border-muted-faint"
    />
  );
}

/** 一组向量行(Translation/Rotation/Scale) */
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
      <span className="w-[62px] shrink-0 text-2xs text-muted">{label}</span>
      <span className="flex min-w-0 flex-1 items-center gap-0.5">
        {values.map((v, i) => (
          <span key={i} className="flex min-w-0 flex-1 items-center gap-0.5">
            <span className="text-2xs text-muted-faint">{labels[i]}</span>
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

function InspectorPanel() {
  const entities = useEditorStore((s) => s.entities);
  const selectedId = useEditorStore((s) => s.selectedId);
  const renameEntity = useEditorStore((s) => s.renameEntity);
  const setTransform = useEditorStore((s) => s.setTransform);
  const componentTypes = useEditorStore((s) => s.componentTypes);
  const loadComponentTypes = useEditorStore((s) => s.loadComponentTypes);
  const addComponent = useEditorStore((s) => s.addComponent);
  const removeComponent = useEditorStore((s) => s.removeComponent);
  const setComponentEnabled = useEditorStore((s) => s.setComponentEnabled);

  const entity = entities.find((e) => e.id === selectedId) ?? null;
  const [nameDraft, setNameDraft] = useState('');
  useEffect(() => setNameDraft(entity?.name ?? ''), [entity?.id, entity?.name]);
  useEffect(() => {
    if (componentTypes.length === 0) void loadComponentTypes();
  }, [componentTypes.length, loadComponentTypes]);

  const commitName = () => {
    const name = nameDraft.trim();
    if (entity && name !== '' && name !== entity.name) void renameEntity(entity.id, name);
    else setNameDraft(entity?.name ?? '');
  };

  const addable = componentTypes.filter((t) => !entity?.components.some((c) => c.type === t.name));

  return (
    <aside
      className="flex w-[280px] shrink-0 flex-col border-l border-line-soft bg-white"
      aria-label="Inspector"
    >
      <div className="shrink-0 px-2 pb-1 pt-2">
        <span className="text-2xs text-muted-faint">Inspector</span>
      </div>
      {!entity ? (
        <p className="px-3 pt-2 text-xs text-muted-faint">在 Hierarchy 选中实体以编辑属性</p>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto pb-2">
          {/* 实体名 */}
          <div className="px-2 pb-1">
            <input
              value={nameDraft}
              onChange={(e) => setNameDraft(e.target.value)}
              onBlur={commitName}
              onKeyDown={(e) => {
                if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
              }}
              className="w-full rounded-md border border-line bg-white px-2 py-1 text-sm text-ink outline-none focus:border-muted-faint"
            />
          </div>

          {/* Transform 节(rotation 为 xyzw 四元数,与后端数据模型一致) */}
          <div className="border-t border-line-soft py-1">
            <p className="px-2 py-0.5 text-2xs font-medium text-ink-soft">Transform</p>
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

          {/* 组件分节 */}
          {entity.components.map((c) => (
            <div key={c.type} className="border-t border-line-soft py-1">
              <div className="flex items-center gap-1.5 px-2 py-0.5">
                <input
                  type="checkbox"
                  checked={c.enabled}
                  title="enabled"
                  onChange={(e) => void setComponentEnabled(entity.id, c.type, e.target.checked)}
                  className="h-3 w-3 shrink-0 accent-ink"
                />
                <span className="min-w-0 flex-1 truncate text-2xs font-medium text-ink-soft">
                  {c.type}
                </span>
                <button
                  type="button"
                  title="Remove component"
                  className={iconBtn}
                  onClick={() => void removeComponent(entity.id, c.type)}
                >
                  <X size={11} strokeWidth={1.8} />
                </button>
              </div>
              <div className="px-2 pl-7 font-mono text-2xs text-muted">
                {Object.keys(c.props).length === 0 && <p className="text-muted-faint">(无属性)</p>}
                {Object.entries(c.props).map(([k, v]) => (
                  <p key={k} className="truncate py-px" title={JSON.stringify(v)}>
                    <span className="text-muted-faint">{k}</span>: {JSON.stringify(v)}
                  </p>
                ))}
              </div>
            </div>
          ))}

          {/* Add Component(注册表驱动) */}
          <div className="border-t border-line-soft px-2 py-2">
            <select
              value=""
              title="Add Component"
              onChange={(e) => {
                const t = e.target.value;
                if (t !== '') void addComponent(entity.id, t);
              }}
              className="w-full rounded-md border border-line bg-white px-2 py-1 text-xs text-ink-soft outline-none"
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
        </div>
      )}
    </aside>
  );
}

// ---------- F Chat(可折叠右 dock) ----------

function ChatDock() {
  const chatMessages = useEditorStore((s) => s.chatMessages);
  const sendChat = useEditorStore((s) => s.sendChat);
  const toggleChat = useEditorStore((s) => s.toggleChat);
  const chatPrefill = useEditorStore((s) => s.chatPrefill);
  const clearChatPrefill = useEditorStore((s) => s.clearChatPrefill);
  const [draft, setDraft] = useState('');
  const [mode, setMode] = useState<ComposerMode>('build');

  // F2 wave.3:Assets 右键「生成」预填(F3 gen-image/gen-model seam)
  useEffect(() => {
    if (chatPrefill !== null) {
      setDraft(chatPrefill);
      clearChatPrefill();
    }
  }, [chatPrefill, clearChatPrefill]);

  const send = () => {
    const text = draft.trim();
    if (text === '') return;
    setDraft('');
    void sendChat(text, mode);
  };

  return (
    <aside
      className="flex w-[300px] shrink-0 flex-col border-l border-line-soft bg-panel"
      aria-label="Chat"
    >
      <div className="flex shrink-0 items-center justify-between px-2 pb-1 pt-2">
        <span className="text-2xs text-muted-faint">Chat</span>
        <button type="button" title="Collapse Chat" className={iconBtn} onClick={toggleChat}>
          <ChevronRight size={13} strokeWidth={1.8} />
        </button>
      </div>
      {/* composer 五模式切换器(04 §3 / 07 §5;multitask 走 swarm 分片执行) */}
      <div className="flex shrink-0 gap-1 px-2 pb-1" data-testid="composer-modes">
        {COMPOSER_MODES.map((m) => (
          <button
            key={m}
            type="button"
            data-mode={m}
            onClick={() => setMode(m)}
            className={cn(
              'rounded-full px-2 py-0.5 text-2xs transition-colors',
              mode === m ? 'bg-ink text-white' : 'bg-white text-ink-soft hover:bg-panel-hover',
            )}
          >
            {m}
          </button>
        ))}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-1">
        {chatMessages.length === 0 && (
          <p className="pt-1 text-xs text-muted-faint">
            发送消息经 /api/forge/mcp/call 实测回显(F3 接入真实 LLM)
          </p>
        )}
        {chatMessages.map((m, i) => (
          <div
            key={i}
            className={cn(
              'my-1 rounded-lg px-2 py-1 text-xs',
              m.role === 'user' && 'ml-6 bg-ink text-white',
              m.role === 'assistant' && 'mr-4 bg-white font-mono text-2xs text-ink-soft shadow-composer',
              m.role === 'error' && 'mr-4 bg-white text-2xs text-accent-blue shadow-composer',
            )}
          >
            {m.role === 'user' && m.mode && m.mode !== 'build' && (
              <span className="mb-0.5 inline-block rounded-full bg-white/20 px-1.5 text-2xs">{m.mode}</span>
            )}
            <div className="whitespace-pre-wrap break-all">{m.text}</div>
            {/* multitask 分片报告卡片(数据来自 /api/forge/swarm/execute 真实响应) */}
            {m.swarm && (
              <div className="mt-1 space-y-0.5" data-testid="swarm-card">
                {m.swarm.shards.map((s) => (
                  <div key={s.shardId} className="flex items-center gap-1 text-2xs">
                    <span
                      className={cn(
                        'inline-block h-1.5 w-1.5 rounded-full',
                        s.status === 'done' ? 'bg-accent-green' : 'bg-red-500',
                      )}
                    />
                    <span>{s.shardId}</span>
                    <span className="text-muted-faint">
                      {s.status} ok={s.okCount} err={s.errorCount}
                    </span>
                  </div>
                ))}
                <div className="border-t border-line-soft pt-0.5 text-2xs text-muted-faint">
                  聚合 {m.swarm.aggregate.succeeded}/{m.swarm.aggregate.totalItems} 成功 ·{' '}
                  {m.swarm.aggregate.failed} 失败 · disjoint={String(m.swarm.aggregate.disjoint)}
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
      <div className="shrink-0 p-2">
        <div className="flex items-center gap-1 rounded-xl bg-white px-2 py-1 shadow-composer">
          <input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') send();
            }}
            placeholder={mode === 'multitask' ? '批量任务:如「给全部关卡块生成碰撞体」' : 'Ask the engine...'}
            className="min-w-0 flex-1 bg-transparent text-sm text-ink outline-none placeholder:text-muted-faint"
          />
          <button type="button" title="Send" className={iconBtn} onClick={send}>
            <Play size={12} strokeWidth={1.8} />
          </button>
        </div>
      </div>
    </aside>
  );
}

// ---------- 七区总装 ----------

export default function EditorView() {
  const loadEntities = useEditorStore((s) => s.loadEntities);
  const refreshSummary = useEditorStore((s) => s.refreshSummary);
  const refreshPlayState = useEditorStore((s) => s.refreshPlayState);
  const loadEvents = useEditorStore((s) => s.loadEvents);
  const chatOpen = useEditorStore((s) => s.chatOpen);
  const centerTab = useEditorStore((s) => s.centerTab);
  const selectedId = useEditorStore((s) => s.selectedId);
  const loadGraphForSelected = useGraphStore((s) => s.loadForSelectedEntity);

  // 进视图即拉一次真实数据(实体 / 摘要 / PIE 状态 / 事件流)
  useEffect(() => {
    void loadEntities();
    void refreshSummary();
    void refreshPlayState();
    void loadEvents();
  }, [loadEntities, refreshSummary, refreshPlayState, loadEvents]);

  // F4 wave.4:切到 NodeGraph 页签,或页签可见时选中实体变化 → 按 Script.graphRef 载图(无 → 空态)
  useEffect(() => {
    if (centerTab === 'nodegraph') void loadGraphForSelected();
  }, [centerTab, selectedId, loadGraphForSelected]);

  return (
    <div className="flex h-full min-h-0 bg-white">
      {/* 左列:A Hierarchy + B Assets */}
      <div className="flex w-[240px] shrink-0 flex-col border-r border-line-soft bg-panel">
        <HierarchyPanel />
        <AssetsPanel />
      </div>

      {/* 中列:C Viewport(G NodeGraph 同位)+ E Workbench */}
      <div className="flex min-w-0 flex-1 flex-col">
        <ViewportPanel />
        <WorkbenchPanel />
      </div>

      {/* D Inspector */}
      <InspectorPanel />

      {/* F Chat 可折叠右 dock */}
      {chatOpen && <ChatDock />}
    </div>
  );
}
